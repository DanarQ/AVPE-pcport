// Export selected source routines and string references for reproducible Xbox RE.
// @category AVPE
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.program.model.listing.Function;
import java.io.*;
import java.util.*;

public class ExportEvidence extends GhidraScript {
    public void run() throws Exception {
        String[] args=getScriptArgs();
        File dir=new File(args[0]); dir.mkdirs();
        DecompInterface decompiler=new DecompInterface();
        decompiler.openProgram(currentProgram);
        Set<Function> selected=new LinkedHashSet<>();
        long[] addresses={0x133340,0xdeb50,0xdea60,0x132e10,0x132ff0};
        for(long address:addresses) {
            Function f=getFunctionContaining(toAddr(address));
            if(f!=null) selected.add(f);
        }
        try(PrintWriter index=new PrintWriter(new File(dir,"string-references.tsv"))) {
            var data=currentProgram.getListing().getDefinedData(true);
            while(data.hasNext()) {
                var d=data.next();
                if(!(d.getValue() instanceof String)) continue;
                String s=(String)d.getValue();
                if(!(s.matches("(?i).*(menu|loading|[.]tbd|[.]xmv|font|crend|crc32|ctbd|haus|cembedded|cpreset|cdefault|clogo|csprite).*"))) continue;
                for(var ref:getReferencesTo(d.getAddress())) {
                    Function f=getFunctionContaining(ref.getFromAddress());
                    index.println(d.getAddress()+"\t"+ref.getFromAddress()+"\t"+s);
                    if(f!=null) selected.add(f);
                }
            }
        }
        for(Function f:selected) {
            monitor.checkCancelled();
            var result=decompiler.decompileFunction(f,30,monitor);
            if(result.decompileCompleted()) {
                try(PrintWriter out=new PrintWriter(new File(dir,f.getEntryPoint()+".c"))) {
                    out.println(result.getDecompiledFunction().getC());
                }
            }
        }
        decompiler.dispose();
        println("Exported "+selected.size()+" source functions");
    }
}
