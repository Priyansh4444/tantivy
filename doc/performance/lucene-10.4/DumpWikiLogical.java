import java.nio.file.Path;
import java.io.BufferedWriter;
import java.io.OutputStreamWriter;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.Comparator;
import org.apache.lucene.index.*;
import org.apache.lucene.search.IndexSearcher;
import org.apache.lucene.store.FSDirectory;
import org.apache.lucene.util.BytesRef;

/** Offline native inventory only: no BM25 override or timed query changes. */
public class DumpWikiLogical {
    record Row(String id, long sort, long norm) {}
    public static void main(String[] args) throws Exception {
        try (var directory=FSDirectory.open(Path.of(args[0])); var reader=DirectoryReader.open(directory)) {
            var output=new PrintWriter(new BufferedWriter(new OutputStreamWriter(System.out),65536));
            if(reader.hasDeletions()) throw new IllegalArgumentException("expected no deletions");
            if(args[1].equals("terms")) {
                var terms=MultiTerms.getTerms(reader,"text");
                var iterator=terms.iterator();
                BytesRef term;
                long count=0,total=0;
                while((term=iterator.next())!=null) {
                    String word=term.utf8ToString();
                    if(!word.matches("[a-z]+")) throw new IllegalArgumentException("term outside lowercase ASCII domain");
                    long ttf=iterator.totalTermFreq();
                    output.println(word+"\t"+iterator.docFreq()+"\t"+ttf);
                    count++; total+=ttf;
                }
                var stats=new IndexSearcher(reader).collectionStatistics("text");
                System.err.println("{\"documents\":"+reader.numDocs()+",\"field_doc_count\":"+stats.docCount()+",\"terms\":"+count+",\"derived_total_tokens\":"+total+",\"serialized_total_tokens\":"+stats.sumTotalTermFreq()+"}");
            } else if(args[1].equals("documents")) {
                var norms=MultiDocValues.getNormValues(reader,"text");
                var sorts=MultiDocValues.getNumericValues(reader,"sort_field");
                var stored=reader.storedFields();
                var rows=new ArrayList<Row>(reader.numDocs());
                for(int doc=0;doc<reader.maxDoc();doc++) {
                    String id=stored.document(doc).get("id");
                    if(id==null || !id.chars().allMatch(c->c<128 && c!='\t' && c!='\r' && c!='\n')) throw new IllegalArgumentException("ID outside ASCII TSV domain");
                    if(!sorts.advanceExact(doc)) throw new IllegalArgumentException("missing sort value");
                    long norm=norms.advanceExact(doc)?norms.longValue():0;
                    if(norm < -128 || norm > 255) throw new IllegalArgumentException("norm outside encoded byte domain");
                    norm &= 0xff;
                    rows.add(new Row(id,sorts.longValue(),norm));
                }
                rows.sort(Comparator.comparing(Row::id));
                String previous=null;
                for(var row:rows) {
                    if(row.id().equals(previous)) throw new IllegalArgumentException("duplicate external ID");
                    output.println(row.id()+"\t"+Long.toUnsignedString(row.sort())+"\t"+row.norm());
                    previous=row.id();
                }
            } else throw new IllegalArgumentException("expected terms or documents mode");
            output.flush();
            if(output.checkError()) throw new java.io.IOException("inventory output failed");
        }
    }
}
