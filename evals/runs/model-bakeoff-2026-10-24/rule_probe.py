import json, subprocess, time, urllib.request, os, signal
from pathlib import Path
HOME=Path.home()
BIN=Path("/Users/a.sorathiya/Documents/Ali/teletype/crates/teletype-desktop/binaries/llama-server")
MODELS={
 "qwen3.5-2b": HOME/".cache/teletype-bench/models/Qwen3.5-2B-IQ4_XS.gguf",
 "qwen3.5-0.8b": HOME/".cache/teletype-bench/models/Qwen3.5-0.8B-Q8_0.gguf",
 "s1-mini": HOME/"Library/Application Support/com.teletype.app/models/s1-mini.gguf",
}
CASES=[
 ("concise","I just wanted to quickly reach out and ask if it would be possible for you to perhaps send over the updated report at some point today."),
 ("structure","For the launch we need to update the docs, notify support, and schedule the announcement."),
 ("tone","hey can you guys fix this stuff ASAP it's kinda blocking us"),
 ("reorder","After checking the logs and confirming the database was healthy, we found that the outage was caused by an expired TLS certificate."),
]
RULE={
 "concise":"Cut every word that does not carry meaning. Keep all facts.",
 "structure":"Render the enumerated items as a lead-in line followed by one '- ' bullet per item.",
 "tone":"Rewrite in a warm, professional workplace tone. Remove slang (hey, guys, stuff, kinda).",
 "reorder":"Lead with the main point. Put the background after it.",
}
def wait(base, key):
    for _ in range(180):
        try:
            r=urllib.request.Request(base+"/health",headers={"Authorization":"Bearer "+key})
            if urllib.request.urlopen(r,timeout=1).status==200: return
        except Exception: pass
        time.sleep(.25)
    raise SystemExit("server did not start")
def chat(base,key,system,user,n=200):
    body=json.dumps({"model":"local","messages":[{"role":"system","content":system},{"role":"user","content":user}],"temperature":0,"max_tokens":n,"chat_template_kwargs":{"enable_thinking":False}}).encode()
    r=urllib.request.Request(base+"/v1/chat/completions",data=body,headers={"Content-Type":"application/json","Authorization":"Bearer "+key})
    j=json.load(urllib.request.urlopen(r,timeout=120))
    return j["choices"][0]["message"]["content"].strip()
port=18200
for name,path in MODELS.items():
    if not path.exists(): print("SKIP",name); continue
    port+=1; key="probe"
    log=open(f"/tmp/probe-{name}.log","w")
    p=subprocess.Popen([str(BIN),"-m",str(path),"--host","127.0.0.1","--port",str(port),"--api-key",key,
        "-c","4096","-fa","on","-ctk","q8_0","-ctv","q8_0","--jinja","--cache-reuse","256","-np","1",
        "--chat-template-kwargs",json.dumps({"enable_thinking":False})],stdout=subprocess.DEVNULL,stderr=log)
    base=f"http://127.0.0.1:{port}"
    try:
        wait(base,key)
        print("="*100); print(name)
        for rule,text in CASES:
            sysmsg=f"Apply this style rule to the text the user provides.\nRULE: {RULE[rule]}\nReturn only the rewritten text."
            out=chat(base,key,sysmsg,"TEXT:\n"+text)
            print(f"\n[{rule}] IN : {text}\n          OUT: {out}")
    finally:
        p.send_signal(signal.SIGTERM); p.wait(timeout=10); log.close()
