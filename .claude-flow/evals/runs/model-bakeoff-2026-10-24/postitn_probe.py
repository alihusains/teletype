import json, subprocess, time, urllib.request, signal, re, statistics
from pathlib import Path
HOME=Path.home()
BIN=Path("/Users/a.sorathiya/Documents/Ali/teletype/crates/teletype-desktop/binaries/llama-server")
MP=HOME/".cache/teletype-bench/models/Qwen3.5-2B-IQ4_XS.gguf"
# What the pipeline hands the model: dictionary + filler removal + ITN done,
# only punctuation/capitalization/grammar left.
CASES=[
 ("c01","i think we should move the meeting to friday at 2 PM","I think we should move the meeting to Friday at 2 PM.",["friday","2","pm"]),
 ("c02","hey john can you send me the quarterly report by thursday","Hey John, can you send me the quarterly report by Thursday?",["john","quarterly","thursday"]),
 ("c03","the new api endpoint is at api.example.com/v2/users","The new API endpoint is at api.example.com/v2/users.",["api.example.com/v2/users"]),
 ("c04","my email is ali.sorathiya@gmail.com","My email is ali.sorathiya@gmail.com.",["ali.sorathiya@gmail.com"]),
 ("c05","we need to buy three things milk eggs and bread","We need to buy three things: milk, eggs and bread.",["three","milk","eggs","bread"]),
 ("c06","lets meet at 4","Let's meet at 4.",["4"]),
 ("c07","the budget is $50,000 for Q3","The budget is $50,000 for Q3.",["50000","q3"]),
 ("c08","i spoke to shivon rourke about the kubernetes migration","I spoke to Shivon Rourke about the Kubernetes migration.",["shivon","rourke","kubernetes"]),
 ("c09","the plan is we ship on monday and then we monitor for a week","The plan is we ship on Monday and then monitor for a week.",["monday","week"]),
 ("c10","the meeting is on october 24 at 10 AM","The meeting is on October 24 at 10 AM.",["october","24","10"]),
 ("c11","please review the pull request number 1234","Please review pull request number 1234.",["1234","pull request"]),
 ("c12","can you add the new api key to the staging environment","Can you add the new API key to the staging environment?",["api","staging"]),
]
LEAN=("You clean up dictated text. Fix punctuation, capitalization, spelling and grammar. "
 "Keep every word, name, number and fact exactly as written; do not add or remove information. "
 "Return only the cleaned text.")
STOP=set("a an the and or but so if then of in on at to for with by from up about into over after before is are was were be been being am do does did it its this that these those i we you he she they me my our your their".split())
NUMWORDS={"zero":0,"one":1,"two":2,"three":3,"four":4,"five":5,"six":6,"seven":7,"eight":8,"nine":9,"ten":10}
FILLERS=re.compile(r"\b(um|uh|er|ah|basically|actually)\b",re.I)
PREAMBLE=re.compile(r"^\s*(here is|here's|sure[,!.]|certainly[,!.]|the cleaned|cleaned text|transformed text)",re.I)
def canon(s):
    s=s.lower().replace("\u2019","'").replace("\u2018","'")
    s=re.sub(r"(?<=\d),(?=\d{3})","",s); s=re.sub(r"(\d)\s*(am|pm)\b",r"\1\2",s)
    toks=re.findall(r"[a-z0-9]+(?:[.@/:#%$-][a-z0-9]+)*",s); out=[]
    for t in toks:
        if t in NUMWORDS or re.fullmatch(r"\d+",t) or (re.match(r"\d",t) and re.search(r"\d",t)): out.append("NUM")
        else: out.append(t)
    return out
def content(s): return [t for t in canon(s) if t not in STOP]
def norm(s): return re.sub(r"\s+"," ",s.strip().lower())
def must_present(must,out):
    o=norm(out).replace(",","").replace("$","")
    return sum(1 for m in must if norm(m).replace(",","").replace("$","") in o or norm(m).replace(",","").replace("$","").replace(" ","") in o.replace(" ",""))/len(must)
def surface(t):
    c=[not FILLERS.search(t), bool(re.match(r"[A-Z]",t.strip())) if t.strip() else False,
       bool(re.search(r"[.!?]\s*$",t.strip())) or "\n- " in t,"```" not in t,not PREAMBLE.match(t)]
    return sum(c)/len(c)
def score(ref,out,must):
    r,o=content(ref),content(out)
    rec=sum(1 for x in set(r) if x in o)/len(set(r)) if r else 1.0
    pre=sum(1 for x in set(o) if x in r)/len(set(o)) if o else 0.0
    return round(100*(0.35*must_present(must,out)+0.30*rec+0.25*pre+0.10*surface(out)),1)
def wait(base,key):
    for _ in range(240):
        try:
            q=urllib.request.Request(base+"/health",headers={"Authorization":"Bearer "+key})
            if urllib.request.urlopen(q,timeout=1).status==200: return
        except Exception: pass
        time.sleep(.25)
    raise SystemExit("no start")
def chat(base,key,sys_,user,n=256):
    b=json.dumps({"model":"local","messages":[{"role":"system","content":sys_},{"role":"user","content":user}],"temperature":0,"max_tokens":n,"chat_template_kwargs":{"enable_thinking":False}}).encode()
    q=urllib.request.Request(base+"/v1/chat/completions",data=b,headers={"Content-Type":"application/json","Authorization":"Bearer "+key})
    return json.load(urllib.request.urlopen(q,timeout=120))["choices"][0]["message"]["content"].strip()
port=18400; key="probe"; log=open("/tmp/postitn.log","w")
p=subprocess.Popen([str(BIN),"-m",str(MP),"--host","127.0.0.1","--port",str(port),"--api-key",key,"-c","4096","-fa","on","-ctk","q8_0","-ctv","q8_0","--jinja","--cache-reuse","256","-np","1","--chat-template-kwargs",json.dumps({"enable_thinking":False})],stdout=subprocess.DEVNULL,stderr=log)
base=f"http://127.0.0.1:{port}"
scores=[]
try:
    wait(base,key)
    for cid,text,ref,must in CASES:
        out=chat(base,key,LEAN,"TEXT:\n"+text)
        s=score(ref,out,must); scores.append(s)
        print(f"[{cid}] {s:5.1f}  {out}")
    print(f"MEAN {statistics.mean(scores):.1f}")
finally:
    p.send_signal(signal.SIGTERM); p.wait(timeout=10); log.close()
