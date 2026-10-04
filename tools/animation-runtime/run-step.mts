/** Run one owned verification command and publish terminal state without polling. */
import { resolve } from "node:path";
interface Options { job_id: string; state: string; log: string; cwd: string; expected_exit: number; command: string[] }
/** Parse named flags followed by the command separator. */
function options(): Options {
    const args = Bun.argv.slice(2); const split = args.indexOf("--");
    if (split < 0) throw new Error("usage: --job-id ID --state PATH --log PATH --cwd PATH [--expected-exit N] -- COMMAND...");
    const values = new Map<string,string>();
    for (let index=0; index<split; index+=2) {
        const key=args[index], value=args[index+1];
        if (!key || !value || !["--job-id","--state","--log","--cwd","--expected-exit"].includes(key)) throw new Error("invalid flags");
        values.set(key,value);
    }
    const required = (key:string):string => {const value=values.get(key); if (!value) throw new Error(`missing ${key}`); return value;};
    const expected_exit=Number(values.get("--expected-exit") ?? "0");
    const command=args.slice(split+1); if (!Number.isInteger(expected_exit) || command.length===0) throw new Error("invalid command/exit");
    return {job_id:required("--job-id"),state:resolve(required("--state")),log:resolve(required("--log")),cwd:resolve(required("--cwd")),expected_exit,command};
}
/** Mirror stdout/stderr to a durable log, keeping no whole-log RAM buffer. */
async function capture(stream:ReadableStream<Uint8Array>,writer:ReturnType<ReturnType<typeof Bun.file>["writer"]>):Promise<void> {
    for await (const chunk of stream) writer.write(chunk);
}
/** Execute one command; the detached Ilium monitor reads the state file. */
async function main():Promise<void> {
    const config=options(); await Bun.write(config.log,"");
    const child=Bun.spawn(config.command,{cwd:config.cwd,stdout:"pipe",stderr:"pipe",stdin:"ignore"});
    await Bun.write(config.state,JSON.stringify({job_id:config.job_id,status:"running",percent:5,message:`Owned verification PID ${child.pid}; see ${config.log}`}));
    console.log(JSON.stringify({type:"progress",job_id:config.job_id,pid:child.pid,state:config.state,log:config.log}));
    const writer=Bun.file(config.log).writer();
    await Promise.all([capture(child.stdout,writer),capture(child.stderr,writer)]);
    const code=await child.exited; await writer.end();
    const ok=code===config.expected_exit;
    await Bun.write(config.state,JSON.stringify({job_id:config.job_id,status:ok?"done":"error",percent:100,message:`Verification exited ${code}; full implementation goal remains active`,...(ok?{}:{error:`Expected exit ${config.expected_exit}; see ${config.log}`})}));
    console.log(JSON.stringify({type:"summary",job_id:config.job_id,exit_code:code,expected_exit:config.expected_exit,ok,log:config.log,state:config.state}));
    process.exitCode=ok?0:1;
}
await main().catch(error=>{console.error(JSON.stringify({type:"error",error:String(error)}));process.exitCode=1;});
