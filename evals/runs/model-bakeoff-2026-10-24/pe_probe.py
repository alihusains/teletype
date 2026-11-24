import json, subprocess, time, urllib.request, signal
from pathlib import Path
HOME=Path.home()
BIN=Path("/Users/a.sorathiya/Documents/Ali/teletype/crates/teletype-desktop/binaries/llama-server")
MP=HOME/".cache/teletype-bench/models/Qwen3.5-2B-IQ4_XS.gguf"
SYS=("You write AI prompts. The user gives you a rough request. You return a finished, reusable prompt that would fulfil it. "
"Keep the user's intent exactly; add the structure and specificity the request implies. Do not answer the request.\n\n"
"Example\nRequest: turn meeting notes into a list of decisions\nPrompt:\n"
"ROLE: You are a meeting analyst.\n"
"TASK: Read the meeting notes below and extract every decision that was made.\n"
"INPUT: <meeting notes>\n"
"CONSTRAINTS: Only include decisions that were explicitly agreed. Do not invent owners or dates. Quote the exact wording where it matters.\n"
"OUTPUT FORMAT: A bullet list, one decision per line, each followed by the evidence sentence it came from.")
CASES=[
 "write a prompt to summarize a customer support ticket",
 "make a prompt for reviewing rust code for safety issues",
 "prompt to turn meeting notes into action items",
 "prompt for generating unit tests for a function",
 "prompt to write a cold email to a recruiter",
 "prompt to explain a codebase to a new engineer",
]
def wait(base,key):
    for _ in range(240):
        try:
            q=urllib.request.Request(base+"/health",headers={"Authorization":"Bearer "+key})
            if urllib.request.urlopen(q,timeout=1).status==200: return
        except Exception: pass
        time.sleep(.25)
    raise SystemExit("no start")
def chat(base,key,sys_,user,n=320):
    b=json.dumps({"model":"local","messages":[{"role":"system","content":sys_},{"role":"user","content":user}],"temperature":0,"max_tokens":n,"chat_template_kwargs":{"enable_thinking":False}}).encode()
    q=urllib.request.Request(base+"/v1/chat/completions",data=b,headers={"Content-Type":"application/json","Authorization":"Bearer "+key})
    return json.load(urllib.request.urlopen(q,timeout=120))["choices"][0]["message"]["content"].strip()
port=18500; key="probe"; log=open("/tmp/pe.log","w")
p=subprocess.Popen([str(BIN),"-m",str(MP),"--host","127.0.0.1","--port",str(port),"--api-key",key,"-c","4096","-fa","on","-ctk","q8_0","-ctv","q8_0","--jinja","--cache-reuse","256","-np","1","--chat-template-kwargs",json.dumps({"enable_thinking":False})],stdout=subprocess.DEVNULL,stderr=log)
base=f"http://127.0.0.1:{port}"
try:
    wait(base,key)
    for c in CASES:
        print("="*90); print("REQ:",c); print(chat(base,key,SYS,"Request: "+c))
finally:
    p.send_signal(signal.SIGTERM); p.wait(timeout=10); log.close()
