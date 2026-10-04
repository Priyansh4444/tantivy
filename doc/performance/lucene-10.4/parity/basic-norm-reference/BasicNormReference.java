import org.apache.lucene.analysis.core.KeywordAnalyzer;
import org.apache.lucene.document.Document;
import org.apache.lucene.document.Field;
import org.apache.lucene.document.FieldType;
import org.apache.lucene.index.DirectoryReader;
import org.apache.lucene.index.IndexOptions;
import org.apache.lucene.index.IndexWriter;
import org.apache.lucene.index.IndexWriterConfig;
import org.apache.lucene.index.MultiDocValues;
import org.apache.lucene.index.Term;
import org.apache.lucene.search.IndexSearcher;
import org.apache.lucene.search.TermQuery;
import org.apache.lucene.search.similarities.BM25Similarity;
import org.apache.lucene.store.ByteBuffersDirectory;
import org.apache.lucene.util.BytesRef;

/** DOCS term fields with norms, deliberately not numeric/date/IP point fields. */
public class BasicNormReference {
  public static void main(String[] args) throws Exception {
    var type = new FieldType();
    type.setIndexOptions(IndexOptions.DOCS);
    type.setTokenized(false);
    type.setOmitNorms(false);
    type.freeze();
    // Opaque binary terms: equality, not their external numeric representation,
    // controls uniqueTermCount. The Rust test checks each field's encoded identity.
    var a = new BytesRef(new byte[] {0, 0, 0, 0, 0, 0, 0, 1});
    var b = new BytesRef(new byte[] {0, 0, 0, 0, 0, 0, 0, 2});
    try (var dir = new ByteBuffersDirectory();
         var writer = new IndexWriter(dir, new IndexWriterConfig(new KeywordAnalyzer()))) {
      for (var terms : new BytesRef[][] {{a, a, b}, {a, b}, {a}, {}}) {
        var document = new Document();
        for (var term : terms) document.add(new Field("value", term, type));
        writer.addDocument(document);
      }
      writer.commit();
      try (var reader = DirectoryReader.open(writer)) {
        var searcher = new IndexSearcher(reader);
        searcher.setSimilarity(new BM25Similarity());
        var stats = searcher.collectionStatistics("value");
        System.out.printf("population=%d tokens=%d dfA=%d dfB=%d%n",
            stats.docCount(), stats.sumTotalTermFreq(),
            reader.docFreq(new Term("value", a)), reader.docFreq(new Term("value", b)));
        var norms = MultiDocValues.getNormValues(reader, "value");
        for (int doc = 0; doc < reader.maxDoc(); doc++)
          System.out.printf("doc=%d norm=%d%n", doc, norms.advanceExact(doc) ? norms.longValue() : 0);
        var query = new TermQuery(new Term("value", a));
        for (int doc = 0; doc < 3; doc++) {
          float score = searcher.explain(query, doc).getValue().floatValue();
          System.out.printf("doc=%d score=%.9g bits=%08x%n", doc, score, Float.floatToRawIntBits(score));
        }
      }
    }
  }
}
