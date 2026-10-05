import java.nio.file.Path;
import java.io.BufferedWriter;
import java.io.OutputStreamWriter;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashSet;
import java.security.MessageDigest;
import java.util.HexFormat;
import com.eclipsesource.json.JsonObject;
import org.apache.lucene.index.*;
import org.apache.lucene.search.IndexSearcher;
import org.apache.lucene.store.FSDirectory;
import org.apache.lucene.util.BytesRef;

/** Offline native inventory only: no BM25 override or timed query changes. */
public class DumpWikiLogical {
    record Row(String id, long sort, long norm) {}
    /** The same byte framing used by the maintained Rust payload_identity walker. */
    static final class CanonicalHash {
        private final MessageDigest hash = MessageDigest.getInstance("SHA-256");
        private final byte[] word = new byte[8];
        CanonicalHash() throws java.security.NoSuchAlgorithmException {}
        void number(long value, int width) {
            for(int i=0;i<width;i++) word[i]=(byte)(value >>> (8*i));
            hash.update(word,0,width);
        }
        void bytes(byte[] bytes,int offset,int length) {
            number(length,8);hash.update(bytes,offset,length);
        }
        void text(String text) {
            byte[] bytes=text.getBytes(java.nio.charset.StandardCharsets.UTF_8);
            bytes(bytes,0,bytes.length);
        }
        String finish() { return HexFormat.of().formatHex(hash.digest()); }
    }
    static void requirePhysical(DirectoryReader reader) {
        if(reader.leaves().size()!=1 || reader.hasDeletions() || reader.maxDoc()!=reader.numDocs())
            throw new IllegalArgumentException("expected one deletion-free segment");
    }
    static String postingsDigest(DirectoryReader reader) throws Exception {
        requirePhysical(reader);
        var terms=MultiTerms.getTerms(reader,"text");
        if(terms==null || !terms.hasPositions()) throw new IllegalArgumentException("missing text positions");
        var iterator=terms.iterator();
        var hash=new CanonicalHash();hash.text("canonical-postings-v1");hash.text("text");
        long termCount=0,postingCount=0,tokenCount=0,positionCount=0;
        boolean[] present=new boolean[reader.maxDoc()];
        BytesRef term;
        while((term=iterator.next())!=null) {
            for(int i=term.offset;i<term.offset+term.length;i++)
                if(term.bytes[i]<'a' || term.bytes[i]>'z') throw new IllegalArgumentException("term outside ASCII domain");
            if(term.length==0) throw new IllegalArgumentException("empty term");
            // Hash the actual BytesRef slice, never its backing array or decoded String.
            hash.bytes(term.bytes,term.offset,term.length);hash.number(iterator.docFreq(),4);
            var postings=iterator.postings(null,PostingsEnum.POSITIONS);
            int observedDf=0;
            for(int doc=postings.nextDoc();doc!=org.apache.lucene.search.DocIdSetIterator.NO_MORE_DOCS;doc=postings.nextDoc()) {
                int frequency=postings.freq();
                if(frequency<=0) throw new IllegalArgumentException("invalid term frequency");
                observedDf++;postingCount++;tokenCount+=frequency;present[doc]=true;
                hash.number(doc,4);hash.number(frequency,4);hash.number(frequency,8);
                for(int i=0;i<frequency;i++) {
                    int position=postings.nextPosition();
                    if(position<0) throw new IllegalArgumentException("missing position");
                    hash.number(position,4);positionCount++;
                }
            }
            if(observedDf!=iterator.docFreq()) throw new IllegalArgumentException("derived DF differs");
            termCount++;
        }
        hash.number(termCount,8);
        long fieldDocs=0;for(boolean value:present) if(value) fieldDocs++;
        var stats=new IndexSearcher(reader).collectionStatistics("text");
        if(stats==null || stats.docCount()!=fieldDocs || stats.sumTotalTermFreq()!=tokenCount)
            throw new IllegalArgumentException("derived text N/TTF differs from actual headers");
        return new JsonObject().add("protocol","wiki-postings-v1").add("sha256",hash.finish())
            .add("terms",termCount).add("postings",postingCount).add("derived_tokens",tokenCount)
            .add("positions",positionCount).add("derived_field_docs",fieldDocs)
            .add("documents",reader.numDocs()).add("field_doc_count",stats.docCount())
            .add("serialized_total_tokens",stats.sumTotalTermFreq()).toString();
    }
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
            } else if(args[1].equals("postings-digest")) {
                output.println(postingsDigest(reader));
            } else if(args[1].equals("documents") || args[1].equals("documents-physical")) {
                boolean physical=args[1].equals("documents-physical");
                if(physical) requirePhysical(reader);
                var norms=MultiDocValues.getNormValues(reader,"text");
                var sorts=MultiDocValues.getNumericValues(reader,"sort_field");
                var stored=reader.storedFields();
                var rows=new ArrayList<Row>(reader.numDocs());
                var ids=new HashSet<String>();
                for(int doc=0;doc<reader.maxDoc();doc++) {
                    var document=stored.document(doc);
                    String id=document.get("id");
                    if(physical && document.getFields("id").length!=1)
                        throw new IllegalArgumentException("expected exactly one stored ID");
                    if(id==null || !id.chars().allMatch(c->c<128 && c!='\t' && c!='\r' && c!='\n')) throw new IllegalArgumentException("ID outside ASCII TSV domain");
                    if(!sorts.advanceExact(doc)) throw new IllegalArgumentException("missing sort value");
                    long norm=norms.advanceExact(doc)?norms.longValue():0;
                    if(norm < -128 || norm > 255) throw new IllegalArgumentException("norm outside encoded byte domain");
                    norm &= 0xff;
                    if(physical) {
                        if(!ids.add(id)) throw new IllegalArgumentException("duplicate external ID");
                        output.println(doc+"\t"+id+"\t"+Long.toUnsignedString(sorts.longValue())+"\t"+norm);
                    } else rows.add(new Row(id,sorts.longValue(),norm));
                }
                rows.sort(Comparator.comparing(Row::id));
                String previous=null;
                for(var row:rows) {
                    if(row.id().equals(previous)) throw new IllegalArgumentException("duplicate external ID");
                    output.println(row.id()+"\t"+Long.toUnsignedString(row.sort())+"\t"+row.norm());
                    previous=row.id();
                }
            } else throw new IllegalArgumentException("expected terms/documents/documents-physical/postings-digest mode");
            output.flush();
            if(output.checkError()) throw new java.io.IOException("inventory output failed");
        }
    }
}
