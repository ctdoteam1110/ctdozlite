// VideoBridge: camera capture, H.264 encode and H.264 decode for Zalo video calls using the
// system's AVFoundation and VideoToolbox, so calls work without a separately installed ffmpeg.
//
// The pipes match what the Rust side previously read from ffmpeg (src/media.rs):
//   VideoBridge list
//       one camera name per line on stdout.
//   VideoBridge capture <camera name | unique id | index | ""> <width> <height> <fps> <bitrate>
//       Annex-B H.264 baseline on stdout, an AUD (00 00 00 01 09 F0) before every access unit and
//       SPS + PPS before every key frame (like x264 `aud=1:repeat-headers=1`).
//   VideoBridge decode <width> <height>
//       Annex-B H.264 on stdin; one width*height*4 RGBA frame (letterboxed, opaque) per decoded
//       picture on stdout.
// Exit codes: 2 bad arguments, 3 camera access denied, 4 no camera / camera failed.

import AVFoundation
import Accelerate
import CoreMedia
import CoreVideo
import Foundation
import VideoToolbox

let stdoutHandle = FileHandle.standardOutput
let outputLock = NSLock()

func emit(_ data: Data) {
    outputLock.lock()
    defer { outputLock.unlock() }
    stdoutHandle.write(data)
}

func logErr(_ s: String) {
    FileHandle.standardError.write((s + "\n").data(using: .utf8)!)
}

// MARK: - Cameras

func allCameras() -> [AVCaptureDevice] {
    var types: [AVCaptureDevice.DeviceType] = [.builtInWideAngleCamera]
    if #available(macOS 14.0, *) {
        types.append(.external)
        types.append(.continuityCamera)
    } else {
        types.append(.externalUnknown)
    }
    return AVCaptureDevice.DiscoverySession(
        deviceTypes: types, mediaType: .video, position: .unspecified
    ).devices
}

func pickCamera(_ wanted: String) -> AVCaptureDevice? {
    let cams = allCameras()
    if !wanted.isEmpty {
        if let d = cams.first(where: { $0.uniqueID == wanted || $0.localizedName == wanted }) {
            return d
        }
        if let i = Int(wanted), i >= 0, i < cams.count {
            return cams[i]
        }
    }
    return cams.first ?? AVCaptureDevice.default(for: .video)
}

// MARK: - Capture + encode

let audNal: [UInt8] = [0, 0, 0, 1, 0x09, 0xF0]
let startCode: [UInt8] = [0, 0, 0, 1]

final class Encoder: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    let width: Int32
    let height: Int32
    let fps: Int32
    let bitrate: Int32
    let session = AVCaptureSession()
    let queue = DispatchQueue(label: "videobridge.capture")
    var compressor: VTCompressionSession?
    var transfer: VTPixelTransferSession?
    var pool: CVPixelBufferPool?
    var lastFrame = CMTime.invalid
    var frameIndex: Int64 = 0

    init(width: Int32, height: Int32, fps: Int32, bitrate: Int32) {
        self.width = width
        self.height = height
        self.fps = max(1, fps)
        self.bitrate = max(64_000, bitrate)
    }

    func start(device: AVCaptureDevice) -> Bool {
        guard let input = try? AVCaptureDeviceInput(device: device), session.canAddInput(input) else {
            return false
        }
        session.beginConfiguration()
        if session.canSetSessionPreset(.vga640x480) {
            session.sessionPreset = .vga640x480
        }
        session.addInput(input)
        let output = AVCaptureVideoDataOutput()
        output.videoSettings = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        ]
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(self, queue: queue)
        guard session.canAddOutput(output) else { return false }
        session.addOutput(output)
        session.commitConfiguration()

        var c: VTCompressionSession?
        let status = VTCompressionSessionCreate(
            allocator: nil, width: width, height: height, codecType: kCMVideoCodecType_H264,
            encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
            outputCallback: nil, refcon: nil, compressionSessionOut: &c)
        guard status == noErr, let comp = c else { return false }
        VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
        VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_ProfileLevel,
                             value: kVTProfileLevel_H264_Baseline_3_0)
        VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_AllowFrameReordering,
                             value: kCFBooleanFalse)
        VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_MaxKeyFrameInterval,
                             value: NSNumber(value: fps))
        VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration,
                             value: NSNumber(value: 1.0))
        VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_ExpectedFrameRate,
                             value: NSNumber(value: fps))
        VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_AverageBitRate,
                             value: NSNumber(value: bitrate))
        let bytesPerSecond = Double(bitrate) * 1.25 / 8.0
        VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_DataRateLimits,
                             value: [NSNumber(value: bytesPerSecond), NSNumber(value: 1.0)] as CFArray)
        VTCompressionSessionPrepareToEncodeFrames(comp)
        compressor = comp

        var t: VTPixelTransferSession?
        VTPixelTransferSessionCreate(allocator: nil, pixelTransferSessionOut: &t)
        if let t = t {
            VTSessionSetProperty(t, key: kVTPixelTransferPropertyKey_ScalingMode,
                                 value: kVTScalingMode_Letterbox)
        }
        transfer = t
        let attrs: [String: Any] = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferWidthKey as String: Int(width),
            kCVPixelBufferHeightKey as String: Int(height),
            kCVPixelBufferIOSurfacePropertiesKey as String: [String: Any](),
        ]
        CVPixelBufferPoolCreate(nil, nil, attrs as CFDictionary, &pool)
        session.startRunning()
        return true
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        guard let comp = compressor, let src = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        let pts = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
        // Throttle the camera's 30 fps to the call's frame rate.
        if lastFrame.isValid,
           CMTimeGetSeconds(CMTimeSubtract(pts, lastFrame)) < 0.92 / Double(fps) {
            return
        }
        lastFrame = pts
        var frame: CVPixelBuffer = src
        if let pool = pool, let t = transfer {
            var dst: CVPixelBuffer?
            if CVPixelBufferPoolCreatePixelBuffer(nil, pool, &dst) == kCVReturnSuccess, let dst = dst,
               VTPixelTransferSessionTransferImage(t, from: src, to: dst) == noErr {
                frame = dst
            }
        }
        frameIndex += 1
        VTCompressionSessionEncodeFrame(
            comp, imageBuffer: frame, presentationTimeStamp: pts, duration: .invalid,
            frameProperties: nil, infoFlagsOut: nil
        ) { status, _, sample in
            guard status == noErr, let sample = sample else { return }
            Encoder.writeAnnexB(sample)
        }
    }

    static func writeAnnexB(_ sample: CMSampleBuffer) {
        guard let block = CMSampleBufferGetDataBuffer(sample) else { return }
        var out = Data(audNal)
        var keyframe = true
        if let atts = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false)
            as? [[CFString: Any]], let first = atts.first {
            keyframe = !((first[kCMSampleAttachmentKey_NotSync] as? Bool) ?? false)
        }
        if keyframe, let fmt = CMSampleBufferGetFormatDescription(sample) {
            var count = 0
            CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                fmt, parameterSetIndex: 0, parameterSetPointerOut: nil, parameterSetSizeOut: nil,
                parameterSetCountOut: &count, nalUnitHeaderLengthOut: nil)
            for i in 0..<count {
                var ptr: UnsafePointer<UInt8>?
                var size = 0
                if CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                    fmt, parameterSetIndex: i, parameterSetPointerOut: &ptr, parameterSetSizeOut: &size,
                    parameterSetCountOut: nil, nalUnitHeaderLengthOut: nil) == noErr, let ptr = ptr {
                    out.append(contentsOf: startCode)
                    out.append(ptr, count: size)
                }
            }
        }
        var total = 0
        var dataPtr: UnsafeMutablePointer<Int8>?
        guard CMBlockBufferGetDataPointer(block, atOffset: 0, lengthAtOffsetOut: nil,
                                          totalLengthOut: &total, dataPointerOut: &dataPtr) == noErr,
              let base = dataPtr else { return }
        let bytes = UnsafeRawPointer(base).assumingMemoryBound(to: UInt8.self)
        var offset = 0
        // VideoToolbox emits AVCC: 4-byte big-endian lengths instead of start codes.
        while offset + 4 <= total {
            let len = Int(bytes[offset]) << 24 | Int(bytes[offset + 1]) << 16
                | Int(bytes[offset + 2]) << 8 | Int(bytes[offset + 3])
            offset += 4
            guard len > 0, offset + len <= total else { break }
            out.append(contentsOf: startCode)
            out.append(bytes + offset, count: len)
            offset += len
        }
        emit(out)
    }
}

func runCapture(_ args: [String]) -> Never {
    guard args.count >= 7, let w = Int32(args[3]), let h = Int32(args[4]),
          let fps = Int32(args[5]), let br = Int32(args[6]) else { exit(2) }
    switch AVCaptureDevice.authorizationStatus(for: .video) {
    case .authorized: break
    case .notDetermined:
        let sem = DispatchSemaphore(value: 0)
        var granted = false
        AVCaptureDevice.requestAccess(for: .video) { ok in granted = ok; sem.signal() }
        sem.wait()
        if !granted { exit(3) }
    default: exit(3)
    }
    guard let device = pickCamera(args[2]) else { exit(4) }
    let encoder = Encoder(width: w, height: h, fps: fps, bitrate: br)
    guard encoder.start(device: device) else { exit(4) }
    // Parent closing our stdout ends the process via SIGPIPE; stdin EOF is not used here.
    withExtendedLifetime(encoder) { RunLoop.main.run() }
    exit(0)
}

/// `test-pattern <width> <height> <frames>`: encodes synthetic colour frames through the same
/// encoder path as a camera, so the pipe can be tested without camera access.
func runTestPattern(_ args: [String]) -> Never {
    guard args.count >= 5, let w = Int32(args[2]), let h = Int32(args[3]), let n = Int(args[4]) else {
        exit(2)
    }
    let enc = Encoder(width: w, height: h, fps: 15, bitrate: 300_000)
    var c: VTCompressionSession?
    guard VTCompressionSessionCreate(
        allocator: nil, width: w, height: h, codecType: kCMVideoCodecType_H264,
        encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
        outputCallback: nil, refcon: nil, compressionSessionOut: &c) == noErr, let comp = c else { exit(4) }
    VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
    VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_Baseline_3_0)
    VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_AllowFrameReordering, value: kCFBooleanFalse)
    VTSessionSetProperty(comp, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: NSNumber(value: 15))
    _ = enc
    for i in 0..<n {
        var pb: CVPixelBuffer?
        CVPixelBufferCreate(nil, Int(w), Int(h), kCVPixelFormatType_32BGRA, nil, &pb)
        guard let buf = pb else { exit(4) }
        CVPixelBufferLockBaseAddress(buf, [])
        let base = CVPixelBufferGetBaseAddress(buf)!.assumingMemoryBound(to: UInt8.self)
        let row = CVPixelBufferGetBytesPerRow(buf)
        for y in 0..<Int(h) {
            for x in 0..<Int(w) {
                let p = base + y * row + x * 4
                // Left half red, right half blue: easy to check after decode.
                if x < Int(w) / 2 { p[0] = 0; p[1] = 0; p[2] = 220 } else { p[0] = 220; p[1] = 0; p[2] = 0 }
                p[3] = 255
            }
        }
        CVPixelBufferUnlockBaseAddress(buf, [])
        let pts = CMTime(value: CMTimeValue(i), timescale: 15)
        VTCompressionSessionEncodeFrame(comp, imageBuffer: buf, presentationTimeStamp: pts,
                                        duration: .invalid, frameProperties: nil, infoFlagsOut: nil) { st, _, sample in
            if st == noErr, let sample = sample { Encoder.writeAnnexB(sample) }
        }
    }
    VTCompressionSessionCompleteFrames(comp, untilPresentationTimeStamp: .invalid)
    exit(0)
}

// MARK: - Decode

final class Decoder {
    let width: Int
    let height: Int
    var sps: Data?
    var pps: Data?
    var format: CMVideoFormatDescription?
    var session: VTDecompressionSession?
    var transfer: VTPixelTransferSession?
    var pool: CVPixelBufferPool?
    var pending: [Data] = []
    var rgba: [UInt8]

    init(width: Int, height: Int) {
        self.width = width
        self.height = height
        rgba = [UInt8](repeating: 0, count: width * height * 4)
        var t: VTPixelTransferSession?
        VTPixelTransferSessionCreate(allocator: nil, pixelTransferSessionOut: &t)
        if let t = t {
            VTSessionSetProperty(t, key: kVTPixelTransferPropertyKey_ScalingMode,
                                 value: kVTScalingMode_Letterbox)
        }
        transfer = t
        let attrs: [String: Any] = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: width,
            kCVPixelBufferHeightKey as String: height,
        ]
        CVPixelBufferPoolCreate(nil, nil, attrs as CFDictionary, &pool)
    }

    func handle(nal: Data) {
        guard let header = nal.first else { return }
        let type = header & 0x1F
        switch type {
        case 7:
            if sps != nal { sps = nal; resetSession() }
        case 8:
            if pps != nal { pps = nal; resetSession() }
        case 9:
            flush()
        case 1, 5:
            // A slice with first_mb_in_slice == 0 starts a new picture even without an AUD.
            if nal.count > 1, nal[nal.startIndex + 1] & 0x80 != 0, !pending.isEmpty {
                flush()
            }
            pending.append(nal)
        default:
            break
        }
    }

    func resetSession() {
        if let s = session { VTDecompressionSessionInvalidate(s) }
        session = nil
        format = nil
    }

    func ensureSession() -> Bool {
        if session != nil { return true }
        guard let sps = sps, let pps = pps else { return false }
        var fmt: CMVideoFormatDescription?
        let status = sps.withUnsafeBytes { spsRaw -> OSStatus in
            pps.withUnsafeBytes { ppsRaw -> OSStatus in
                let ptrs: [UnsafePointer<UInt8>] = [
                    spsRaw.bindMemory(to: UInt8.self).baseAddress!,
                    ppsRaw.bindMemory(to: UInt8.self).baseAddress!,
                ]
                let sizes = [sps.count, pps.count]
                return CMVideoFormatDescriptionCreateFromH264ParameterSets(
                    allocator: nil, parameterSetCount: 2, parameterSetPointers: ptrs,
                    parameterSetSizes: sizes, nalUnitHeaderLength: 4, formatDescriptionOut: &fmt)
            }
        }
        guard status == noErr, let f = fmt else { return false }
        var s: VTDecompressionSession?
        guard VTDecompressionSessionCreate(
            allocator: nil, formatDescription: f, decoderSpecification: nil,
            imageBufferAttributes: nil, outputCallback: nil, decompressionSessionOut: &s) == noErr,
            let sess = s else { return false }
        format = f
        session = sess
        return true
    }

    func flush() {
        defer { pending.removeAll(keepingCapacity: true) }
        guard !pending.isEmpty, ensureSession(), let sess = session, let fmt = format else { return }
        var avcc = Data()
        for nal in pending {
            var len = UInt32(nal.count).bigEndian
            withUnsafeBytes(of: &len) { avcc.append(contentsOf: $0) }
            avcc.append(nal)
        }
        let count = avcc.count
        var block: CMBlockBuffer?
        guard CMBlockBufferCreateWithMemoryBlock(
            allocator: nil, memoryBlock: nil, blockLength: count, blockAllocator: nil,
            customBlockSource: nil, offsetToData: 0, dataLength: count, flags: 0,
            blockBufferOut: &block) == noErr, let blk = block else { return }
        let copied = avcc.withUnsafeBytes {
            CMBlockBufferReplaceDataBytes(with: $0.baseAddress!, blockBuffer: blk,
                                          offsetIntoDestination: 0, dataLength: count)
        }
        guard copied == noErr else { return }
        var sample: CMSampleBuffer?
        var sizes = [count]
        guard CMSampleBufferCreateReady(
            allocator: nil, dataBuffer: blk, formatDescription: fmt, sampleCount: 1,
            sampleTimingEntryCount: 0, sampleTimingArray: nil, sampleSizeEntryCount: 1,
            sampleSizeArray: &sizes, sampleBufferOut: &sample) == noErr, let smp = sample else { return }
        let status = VTDecompressionSessionDecodeFrame(
            sess, sampleBuffer: smp, flags: [], infoFlagsOut: nil
        ) { status, _, image, _, _ in
            guard status == noErr, let image = image else { return }
            self.output(image)
        }
        if status == kVTInvalidSessionErr {
            resetSession()
        }
    }

    func output(_ image: CVImageBuffer) {
        guard let pool = pool, let t = transfer else { return }
        var dst: CVPixelBuffer?
        guard CVPixelBufferPoolCreatePixelBuffer(nil, pool, &dst) == kCVReturnSuccess, let dst = dst else {
            return
        }
        // Letterbox bars must be black, not whatever the pool buffer held.
        CVPixelBufferLockBaseAddress(dst, [])
        if let base = CVPixelBufferGetBaseAddress(dst) {
            memset(base, 0, CVPixelBufferGetBytesPerRow(dst) * CVPixelBufferGetHeight(dst))
        }
        CVPixelBufferUnlockBaseAddress(dst, [])
        guard VTPixelTransferSessionTransferImage(t, from: image, to: dst) == noErr else { return }
        CVPixelBufferLockBaseAddress(dst, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(dst, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(dst) else { return }
        let rowBytes = CVPixelBufferGetBytesPerRow(dst)
        let src = base.assumingMemoryBound(to: UInt8.self)
        rgba.withUnsafeMutableBufferPointer { out in
            for y in 0..<height {
                let row = src + y * rowBytes
                let o = y * width * 4
                for x in 0..<width {
                    let p = row + x * 4
                    out[o + x * 4] = p[2]
                    out[o + x * 4 + 1] = p[1]
                    out[o + x * 4 + 2] = p[0]
                    out[o + x * 4 + 3] = 255
                }
            }
        }
        emit(Data(rgba))
    }
}

/// Splits an Annex-B byte stream into NAL units; the trailing (possibly incomplete) unit stays
/// buffered until the next start code or EOF.
struct NalSplitter {
    var buffer = Data()

    mutating func push(_ data: Data, _ onNal: (Data) -> Void) {
        buffer.append(data)
        var starts: [(Int, Int)] = []  // (start code index, payload index)
        let bytes = [UInt8](buffer)
        var i = 0
        while i + 3 <= bytes.count {
            if bytes[i] == 0, bytes[i + 1] == 0 {
                if bytes[i + 2] == 1 {
                    starts.append((i, i + 3)); i += 3; continue
                }
                if i + 4 <= bytes.count, bytes[i + 2] == 0, bytes[i + 3] == 1 {
                    starts.append((i, i + 4)); i += 4; continue
                }
            }
            i += 1
        }
        guard starts.count >= 2 else { return }
        for k in 0..<(starts.count - 1) {
            let payload = starts[k].1
            let end = starts[k + 1].0
            if end > payload { onNal(Data(bytes[payload..<end])) }
        }
        buffer = Data(bytes[starts[starts.count - 1].0...])
    }

    mutating func finish(_ onNal: (Data) -> Void) {
        let bytes = [UInt8](buffer)
        var p = 0
        while p < bytes.count, bytes[p] == 0 { p += 1 }
        if p < bytes.count, bytes[p] == 1 { p += 1 }
        if p < bytes.count { onNal(Data(bytes[p...])) }
        buffer.removeAll()
    }
}

func runDecode(_ args: [String]) -> Never {
    guard args.count >= 4, let w = Int(args[2]), let h = Int(args[3]), w > 0, h > 0,
          w <= 4096, h <= 4096 else { exit(2) }
    let decoder = Decoder(width: w, height: h)
    var splitter = NalSplitter()
    let input = FileHandle.standardInput
    while true {
        let chunk = input.availableData
        if chunk.isEmpty { break }
        splitter.push(chunk) { decoder.handle(nal: $0) }
    }
    splitter.finish { decoder.handle(nal: $0) }
    decoder.flush()
    exit(0)
}

// MARK: - Main

let argv = CommandLine.arguments
guard argv.count >= 2 else { exit(2) }
switch argv[1] {
case "list":
    for cam in allCameras() {
        print(cam.localizedName)
    }
    exit(0)
case "capture":
    runCapture(argv)
case "decode":
    runDecode(argv)
case "test-pattern":
    runTestPattern(argv)
default:
    exit(2)
}
