import java.nio.file.Path;
import org.apache.lucene.analysis.CharArraySet;
import org.apache.lucene.analysis.standard.StandardAnalyzer;
import org.apache.lucene.document.*;
import org.apache.lucene.index.*;
import org.apache.lucene.store.FSDirectory;

/** Tiny native norm fixture: one empty text and one high encoded norm. */
public class BuildWikiFixture {
    public static void main(String[] args) throws Exception {
        java.nio.file.Files.createDirectory(Path.of(args[0]));
        try(var directory=FSDirectory.open(Path.of(args[0]));
            var writer=new IndexWriter(directory,new IndexWriterConfig(new StandardAnalyzer(CharArraySet.EMPTY_SET)))) {
            for(int i=0;i<2;i++) {
                var doc=new Document();
                doc.add(new StoredField("id",i==0?"empty":"high"));
                doc.add(new TextField("text",i==0?"":"alpha ".repeat(65000),Field.Store.NO));
                doc.add(new NumericDocValuesField("sort_field",i));
                writer.addDocument(doc);
            }
            writer.commit();writer.forceMerge(1);writer.commit();
        }
    }
}
