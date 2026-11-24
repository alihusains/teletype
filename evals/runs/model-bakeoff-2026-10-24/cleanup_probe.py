import json, subprocess, time, urllib.request, signal, re, statistics
from pathlib import Path
HOME=Path.home()
BIN=Path("/Users/a.sorathiya/Documents/Ali/teletype/crates/teletype-desktop/binaries/llama-server")
MODELS={
 "qwen3.5-2b": HOME/".cache/teletype-bench/models/Qwen3.5-2B-IQ4_XS.gguf",
 "qwen3.5-0.8b": HOME/".cache/teletype-bench/models/Qwen3.5-0.8B-Q8_0.gguf",
 "s1-mini": HOME/"Library/Application Support/com.teletype.app/models/s1-mini.gguf",
}
CLEANUP=[
 ("c01","um so i think we should uh move the meeting to friday at 2pm","I think we should move the meeting to Friday at 2 PM.",["friday","2","pm"]),
 ("c02","hey john can you send me the quarterly report by thursday","Hey John, can you send me the quarterly report by Thursday?",["john","quarterly","thursday"]),
 ("c03","the new api endpoint is at api dot example dot com slash v2 slash users","The new API endpoint is at api.example.com/v2/users.",["api.example.com/v2/users"]),
 ("c04","my email is ali dot sorathiya at gmail dot com","My email is ali.sorathiya@gmail.com.",["ali.sorathiya@gmail.com"]),
 ("c05","we need to buy three things milk eggs and bread","We need to buy three things: milk, eggs and bread.",["three","milk","eggs","bread"]),
 ("c06","lets meet at three actually no make that four","Let's meet at 4.",["4"]),
 ("c07","the budget is fifty thousand dollars for q three","The budget is $50,000 for Q3.",["50000","q3"]),
 ("c08","i spoke to shivon rourke about the kubernetes migration","I spoke to Shivon Rourke about the Kubernetes migration.",["shivon","rourke","kubernetes"]),
 ("c09","so basically the plan is we ship on monday and then we monitor for a week","The plan is we ship on Monday and then monitor for a week.",["monday","week"]),
 ("c10","the meeting is on october twenty fourth at ten a m","The meeting is on October 24 at 10 AM.",["october","24","10"]),
 ("c11","please review the pull request number one two three four","Please review pull request number 1234.",["1234","pull request"]),
 ("c12","yeah um can you add the new api key to the staging environment","Can you add the new API key to the staging environment?",["api","staging"]),
]
LEAN=("You clean up dictated text. Fix punctuation, capitalization, spelling and grammar. "
 "Remove filler words (um, uh, er, like, you know, basically). Apply self-corrections: when the speaker "
 "corrects themselves, keep only the corrected version. Convert spoken numbers, dates, times, currency, "
 "URLs and emails into written form. Preserve every name, number, fact and wording choice that is already "
 "correct. Do not add or remove information. Return only the cleaned text.")
S1SYS="You are a writing assistant that cleans up dictated text."
STOP=set("a an the and or but so if then of in on at to for with by from up about into over after before is are was were be been being am do does did it its this that these those i we you he she they me my our your their".split())
NUMWORDS={"zero":0,"one":1,"two":2,"three":3,"four":4,"five":5,"six":6,"seven":7,"eight":8,"nine":9,"ten":10,"eleven":11,"twelve":12,"thirteen":13,"fourteen":14,"fifteen":15,"sixteen":16,"seventeen":17,"eighteen":18,"nineteen":19,"twenty":20,"thirty":30,"forty":40,"fifty":50,"sixty":60,"seventy":70,"eighty":80,"ninety":90,"hundred":100,"thousand":1000,"million":1000000}
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
port=18300
for name,path in MODELS.items():
    if not path.exists(): print("SKIP",name); continue
    port+=1; key="probe"; log=open(f"/tmp/clean-{name}.log","w")
    p=subprocess.Popen([str(BIN),"-m",str(path),"--host","127.0.0.1","--port",str(port),"--api-key",key,"-c","4096","-fa","on","-ctk","q8_0","-ctv","q8_0","--jinja","--cache-reuse","256","-np","1","--chat-template-kwargs",json.dumps({"enable_thinking":False})],stdout=subprocess.DEVNULL,stderr=log)
    base=f"http://127.0.0.1:{port}"
    try:
        wait(base,key); print("="*100); print(name)
        scores=[]
        for cid,text,ref,must in CLEANUP:
            user="TEXT:\n"+text
            out=chat(base,key,LEAN,user)
            s=score(ref,out,must); scores.append(s)
            print(f"[{cid}] {s:5.1f}  {out}")
        print(f"MEAN {statistics.mean(scores):.1f}")
    finally:
        p.send_signal(signal.SIGTERM); p.wait(timeout=10); log.close()
