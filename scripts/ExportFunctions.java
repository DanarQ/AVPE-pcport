// @category AVPE
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import java.io.*;

public class ExportFunctions extends GhidraScript {
    public void run() throws Exception {
        String[] args=getScriptArgs();
        File dir=new File(args[0]); dir.mkdirs();
        DecompInterface decompiler=new DecompInterface();
        decompiler.openProgram(currentProgram);
        for(int i=1;i<args.length;i++) {
            var address=toAddr(Long.parseUnsignedLong(args[i].replace("0x",""),16));
            var f=getFunctionContaining(address);
            if(f==null) { disassemble(address); f=createFunction(address,"avpe_"+address); }
            if(f==null) { println("No function at "+address);continue; }
            var result=decompiler.decompileFunction(f,30,monitor);
            if(result.decompileCompleted()) {
                try(PrintWriter out=new PrintWriter(new File(dir,f.getEntryPoint()+".c"))) {
                    out.println(result.getDecompiledFunction().getC());
                }
            }
        }
        decompiler.dispose();
    }
}
