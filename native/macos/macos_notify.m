#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>
#import <objc/runtime.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static BOOL ctdozlite_has_app_bundle(void) {
    NSBundle *main = [NSBundle mainBundle];
    NSString *bid = main.bundleIdentifier;
    if (bid == nil || bid.length == 0) {
        return NO;
    }
    NSString *ext = main.bundleURL.pathExtension;
    return ext != nil && [ext caseInsensitiveCompare:@"app"] == NSOrderedSame;
}

@interface CtdozNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@end

@implementation CtdozNotificationDelegate

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
       willPresentNotification:(UNNotification *)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions options))completionHandler {
    (void)center;
    UNNotificationPresentationOptions opts =
        UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionList;
    if (notification.request.content.sound != nil) {
        opts |= UNNotificationPresentationOptionSound;
    }
    completionHandler(opts);
}

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
didReceiveNotificationResponse:(UNNotificationResponse *)response
         withCompletionHandler:(void (^)(void))completionHandler {
    (void)center;
    (void)response;
    dispatch_async(dispatch_get_main_queue(), ^{
        NSApplication *app = [NSApplication sharedApplication];
        [app unhide:nil];
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        [app activateIgnoringOtherApps:YES];
#pragma clang diagnostic pop
        for (NSWindow *win in [app windows]) {
            if ([win isMiniaturized]) {
                [win deminiaturize:nil];
            }
            if ([win canBecomeKeyWindow]) {
                [win makeKeyAndOrderFront:nil];
            }
        }
    });
    completionHandler();
}

@end

static CtdozNotificationDelegate *g_delegate = nil;

static UNUserNotificationCenter *ctdozlite_ensure_center(void) {
    if (!ctdozlite_has_app_bundle()) {
        return nil;
    }
    @try {
        UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
        static dispatch_once_t onceToken;
        dispatch_once(&onceToken, ^{
            g_delegate = [[CtdozNotificationDelegate alloc] init];
            center.delegate = g_delegate;
        });
        return center;
    } @catch (NSException *ex) {
        (void)ex;
        return nil;
    }
}

void ctdozlite_macos_notify_setup(int request_if_undetermined) {
    @autoreleasepool {
        UNUserNotificationCenter *center = ctdozlite_ensure_center();
        if (center == nil || !request_if_undetermined) {
            return;
        }
        [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
            if (settings.authorizationStatus == UNAuthorizationStatusNotDetermined) {
                UNAuthorizationOptions opts =
                    UNAuthorizationOptionAlert | UNAuthorizationOptionSound | UNAuthorizationOptionBadge;
                [center requestAuthorizationWithOptions:opts completionHandler:^(BOOL granted, NSError *error) {
                    (void)granted;
                    (void)error;
                }];
            }
        }];
    }
}

static void ctdozlite_post_request(UNUserNotificationCenter *center,
                                   NSString *title,
                                   NSString *body,
                                   NSString *soundName) {
    UNMutableNotificationContent *content = [[UNMutableNotificationContent alloc] init];
    content.title = title ?: @"";
    content.body = body ?: @"";
    if (soundName != nil && soundName.length > 0) {
        content.sound = [UNNotificationSound defaultSound];
    }
    NSString *ident = [[NSUUID UUID] UUIDString];
    UNNotificationRequest *req =
        [UNNotificationRequest requestWithIdentifier:ident content:content trigger:nil];
    [center addNotificationRequest:req withCompletionHandler:^(NSError *error) {
        (void)error;
    }];
}

int ctdozlite_macos_notify(const char *title_utf8,
                           const char *body_utf8,
                           const char *sound_utf8) {
    @autoreleasepool {
        UNUserNotificationCenter *center = ctdozlite_ensure_center();
        if (center == nil) {
            return 0;
        }
        NSString *title = title_utf8 ? [NSString stringWithUTF8String:title_utf8] : @"";
        NSString *body = body_utf8 ? [NSString stringWithUTF8String:body_utf8] : @"";
        NSString *sound = (sound_utf8 && sound_utf8[0] != '\0')
                              ? [NSString stringWithUTF8String:sound_utf8]
                              : nil;
        [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
            UNAuthorizationStatus status = settings.authorizationStatus;
            if (status == UNAuthorizationStatusAuthorized ||
                status == UNAuthorizationStatusProvisional) {
                ctdozlite_post_request(center, title, body, sound);
            } else if (status == UNAuthorizationStatusNotDetermined) {
                UNAuthorizationOptions opts =
                    UNAuthorizationOptionAlert | UNAuthorizationOptionSound | UNAuthorizationOptionBadge;
                [center requestAuthorizationWithOptions:opts completionHandler:^(BOOL granted, NSError *error) {
                    (void)error;
                    if (granted) {
                        ctdozlite_post_request(center, title, body, sound);
                    }
                }];
            }
        }];
        return 1;
    }
}

static BOOL ctdozlite_reopen_handler(id self, SEL _cmd, NSApplication *sender, BOOL flag) {
    (void)self;
    (void)_cmd;
    (void)flag;
    for (NSWindow *win in [sender windows]) {
        NSString *title = [win title];
        if (title != nil &&
            ([title isEqualToString:@"ctdozlite"] ||
             ([title hasPrefix:@"("] && [title hasSuffix:@") ctdozlite"]))) {
            if ([win isMiniaturized]) {
                [win deminiaturize:nil];
            }
            [win makeKeyAndOrderFront:nil];
            break;
        }
    }
    [sender unhide:nil];
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    [sender activateIgnoringOtherApps:YES];
#pragma clang diagnostic pop
    return YES;
}

void ctdozlite_macos_set_appearance(void *win_ptr, void *view_ptr, int dark_mode) {
    @autoreleasepool {
        NSAppearanceName name = dark_mode ? NSAppearanceNameDarkAqua : NSAppearanceNameAqua;
        NSAppearance *appearance = [NSAppearance appearanceNamed:name];
        if (appearance == nil) {
            return;
        }
        if (win_ptr != NULL) {
            NSWindow *win = (__bridge NSWindow *)win_ptr;
            [win setAppearance:appearance];
        }
        if (view_ptr != NULL) {
            NSView *view = (__bridge NSView *)view_ptr;
            [view setAppearance:appearance];
        }
    }
}

static _Atomic int64_t g_macos_paste_ms = 0;
static id g_macos_paste_monitor = nil;

static int64_t ctdozlite_uptime_ms(void) {
    return (int64_t)([[NSProcessInfo processInfo] systemUptime] * 1000.0);
}

void ctdozlite_macos_install_reopen_handler(void) {
    @autoreleasepool {
        static dispatch_once_t onceToken;
        dispatch_once(&onceToken, ^{
            id delegate = [[NSApplication sharedApplication] delegate];
            if (delegate != nil) {
                Class cls = object_getClass(delegate);
                SEL sel = @selector(applicationShouldHandleReopen:hasVisibleWindows:);
                if (!class_respondsToSelector(cls, sel)) {
                    NSString *enc = [NSString stringWithFormat:@"%s@:@%s", @encode(BOOL), @encode(BOOL)];
                    class_addMethod(cls, sel, (IMP)ctdozlite_reopen_handler, [enc UTF8String]);
                }
            }
            g_macos_paste_monitor = [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskKeyDown
                                                                          handler:^NSEvent *(NSEvent *event) {
                NSEventModifierFlags flags =
                    [event modifierFlags] & NSEventModifierFlagDeviceIndependentFlagsMask;
                BOOL is_cmd_only = (flags & NSEventModifierFlagCommand) != 0 &&
                                   (flags & (NSEventModifierFlagControl | NSEventModifierFlagOption)) == 0;
                if (is_cmd_only && ![event isARepeat]) {
                    NSString *chars = [event charactersIgnoringModifiers];
                    if ([event keyCode] == 9 ||
                        (chars != nil && [chars caseInsensitiveCompare:@"v"] == NSOrderedSame)) {
                        atomic_store(&g_macos_paste_ms, ctdozlite_uptime_ms());
                    }
                }
                return event;
            }];
        });
    }
}

int ctdozlite_macos_take_paste_triggered(void) {
    int64_t ts = atomic_exchange(&g_macos_paste_ms, 0);
    if (ts <= 0) {
        return 0;
    }
    int64_t delta = ctdozlite_uptime_ms() - ts;
    return (delta >= 0 && delta <= 300) ? 1 : 0;
}

static NSData *ctdozlite_nsimage_to_png(NSImage *img) {
    if (img == nil) {
        return nil;
    }
    CGImageRef cg = [img CGImageForProposedRect:NULL context:nil hints:nil];
    if (cg != NULL) {
        NSBitmapImageRep *rep = [[NSBitmapImageRep alloc] initWithCGImage:cg];
        if (rep != nil) {
            return [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
        }
    }
    NSData *tiff = [img TIFFRepresentation];
    if (tiff != nil && tiff.length > 0) {
        NSBitmapImageRep *rep = [NSBitmapImageRep imageRepWithData:tiff];
        if (rep != nil) {
            return [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
        }
    }
    return nil;
}

uint8_t *ctdozlite_macos_clipboard_image_png(size_t *out_len) {
    if (out_len == NULL) {
        return NULL;
    }
    *out_len = 0;
    @autoreleasepool {
        NSPasteboard *pb = [NSPasteboard generalPasteboard];
        if (pb == nil) {
            return NULL;
        }
        NSData *pngData = [pb dataForType:NSPasteboardTypePNG];
        if (pngData == nil || pngData.length <= 8) {
            NSArray<NSPasteboardType> *directTypes = @[
                @"public.jpeg",
                @"public.heic",
                @"org.webmproject.webp",
                @"com.compuserve.gif",
            ];
            for (NSPasteboardType t in directTypes) {
                NSData *raw = [pb dataForType:t];
                if (raw != nil && raw.length > 8) {
                    NSImage *img = [[NSImage alloc] initWithData:raw];
                    pngData = ctdozlite_nsimage_to_png(img);
                    if (pngData != nil && pngData.length > 8) {
                        break;
                    }
                }
            }
        }
        if (pngData == nil || pngData.length <= 8) {
            BOOL hasRtfAndText = ([pb dataForType:NSPasteboardTypeRTF] != nil) &&
                                 ([pb dataForType:NSPasteboardTypeString] != nil);
            if (!hasRtfAndText) {
                NSData *tiff = [pb dataForType:NSPasteboardTypeTIFF];
                if (tiff != nil && tiff.length > 8) {
                    NSBitmapImageRep *rep = [NSBitmapImageRep imageRepWithData:tiff];
                    if (rep != nil) {
                        pngData = [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
                    }
                    if (pngData == nil || pngData.length <= 8) {
                        NSImage *img = [[NSImage alloc] initWithData:tiff];
                        pngData = ctdozlite_nsimage_to_png(img);
                    }
                }
            }
        }
        if ((pngData == nil || pngData.length <= 8) && [pb dataForType:NSPasteboardTypeString] == nil) {
            if ([pb canReadObjectForClasses:@[[NSImage class]] options:nil]) {
                NSImage *img = [[NSImage alloc] initWithPasteboard:pb];
                pngData = ctdozlite_nsimage_to_png(img);
            }
        }
        if (pngData == nil || pngData.length <= 8) {
            return NULL;
        }
        uint8_t *buf = (uint8_t *)malloc(pngData.length);
        if (buf == NULL) {
            return NULL;
        }
        memcpy(buf, pngData.bytes, pngData.length);
        *out_len = pngData.length;
        return buf;
    }
}

char *ctdozlite_macos_clipboard_file_paths(void) {
    @autoreleasepool {
        NSPasteboard *pb = [NSPasteboard generalPasteboard];
        if (pb == nil) {
            return NULL;
        }
        NSDictionary *opts = @{ NSPasteboardURLReadingFileURLsOnlyKey: @YES };
        NSArray *urls = [pb readObjectsForClasses:@[[NSURL class]] options:opts];
        if (urls == nil || urls.count == 0) {
            return NULL;
        }
        NSMutableArray<NSString *> *paths = [NSMutableArray arrayWithCapacity:urls.count];
        for (id obj in urls) {
            if ([obj isKindOfClass:[NSURL class]]) {
                NSURL *u = (NSURL *)obj;
                if ([u isFileURL] && u.path.length > 0) {
                    [paths addObject:u.path];
                }
            }
        }
        if (paths.count == 0) {
            return NULL;
        }
        NSString *joined = [paths componentsJoinedByString:@"\n"];
        const char *utf8 = [joined UTF8String];
        return utf8 != NULL ? strdup(utf8) : NULL;
    }
}

void ctdozlite_macos_free_buf(void *ptr) {
    free(ptr);
}
