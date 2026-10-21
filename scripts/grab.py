from mitmproxy import io
want = ["get-last-msgs","friend/getprofiles","profile/me-v2","login/setting","hiddenconvers"]
with open("zalo.flow","rb") as f:
    try:
        for fl in io.FlowReader(f).stream():
            if not hasattr(fl,"request"): continue
            u = fl.request.pretty_url
            if any(w in u for w in want):
                print("="*72)
                print(fl.request.method, u[:400])
                b = fl.request.get_text() or ""
                if b: print("REQ BODY:", b[:400])
                r = (fl.response.get_text() or "") if fl.response else ""
                print("RESP:", r[:700])
    except Exception as e: print("err",e)
