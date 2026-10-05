import java.nio.file.Path;
import org.apache.lucene.analysis.CharArraySet;
import org.apache.lucene.analysis.standard.StandardAnalyzer;
import org.apache.lucene.document.*;
import org.apache.lucene.index.*;
import org.apache.lucene.store.FSDirectory;
import com.eclipsesource.json.Json;

/** Tiny native norm fixture: one empty text and one high encoded norm. */
public class BuildWikiFixture {
    public static void main(String[] args) throws Exception {
        java.nio.file.Files.createDirectory(Path.of(args[0]));
        try(var directory=FSDirectory.open(Path.of(args[0]));
            var writer=new IndexWriter(directory,new IndexWriterConfig(new StandardAnalyzer(CharArraySet.EMPTY_SET)))) {
            if(args.length==2 && args[1].equals("--jsonl")) {
                try(var input=new java.io.BufferedReader(new java.io.InputStreamReader(System.in))) {
                    String line;
                    while((line=input.readLine())!=null) {
                        var row=Json.parse(line).asObject();
                        var doc=new Document();
                        doc.add(new StoredField("id",row.get("id").asString()));
                        doc.add(new TextField("text",row.get("text").asString(),Field.Store.NO));
                        // Preserve the original u64 bits in signed Java doc values.
                        doc.add(new NumericDocValuesField("sort_field",Long.parseUnsignedLong(row.get("sort_field").toString())));
                        writer.addDocument(doc);
                    }
                }
            } else if(args.length==1) for(int i=0;i<2;i++) {
                var doc=new Document();
                doc.add(new StoredField("id",i==0?"empty":"high"));
                doc.add(new TextField("text",i==0?"":"alpha ".repeat(65000),Field.Store.NO));
                doc.add(new NumericDocValuesField("sort_field",i));
                writer.addDocument(doc);
            } else throw new IllegalArgumentException("Usage: INDEX [--jsonl]");
            writer.commit();writer.forceMerge(1);writer.commit();
        }
    }
}
