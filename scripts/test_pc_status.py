from mitmproxy import io
for fl in io.FlowReader(open("zalo.flow","rb")).stream():
    if hasattr(fl,"request") and "pc-status" in fl.request.pretty_url:
        print(fl.request.method, fl.request.pretty_url)
        print("STATUS:", fl.response.status_code if fl.response else None)
        print("RESP:", fl.response.get_text() if fl.response else None)
