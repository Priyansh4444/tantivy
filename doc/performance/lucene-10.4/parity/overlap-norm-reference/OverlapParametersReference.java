import java.util.Arrays;
import org.apache.lucene.document.*;
import org.apache.lucene.index.*;
import org.apache.lucene.search.*;
import org.apache.lucene.search.similarities.BM25Similarity;
import org.apache.lucene.store.ByteBuffersDirectory;
import org.apache.lucene.util.Version;

/** Index-time overlaps and query-time parameters are independent native contracts. */
public class OverlapParametersReference {
  static long[] norms(DirectoryReader reader) throws Exception {
    var values = reader.leaves().get(0).reader().getNormValues("text");
    var result = new long[reader.maxDoc()];
    for (int doc = 0; doc < result.length; doc++) {
      if (!values.advanceExact(doc)) throw new AssertionError("missing norm");
      result[doc] = values.longValue();
    }
    return result;
  }
  static void run(boolean indexDiscount) throws Exception {
    FieldType type = new FieldType();
    type.setTokenized(true);
    type.setIndexOptions(IndexOptions.DOCS_AND_FREQS_AND_POSITIONS);
    type.freeze();
    var first = new OverlapNormReference.Tok[] {
      new OverlapNormReference.Tok("alpha",0),
      new OverlapNormReference.Tok("synonym",0),
      new OverlapNormReference.Tok("beta",1)};
    var second = new OverlapNormReference.Tok[] {
      new OverlapNormReference.Tok("alpha",0),
      new OverlapNormReference.Tok("beta",1)};
    try (var directory = new ByteBuffersDirectory()) {
      var indexSimilarity = indexDiscount ? new BM25Similarity() : new BM25Similarity(false);
      var config = new IndexWriterConfig().setSimilarity(indexSimilarity).setMergePolicy(NoMergePolicy.INSTANCE);
      try (var writer = new IndexWriter(directory,config)) {
        for (var tokens : new OverlapNormReference.Tok[][] {first,second}) {
          var document = new Document();
          document.add(new Field("text",new OverlapNormReference.Tokens(tokens),type));
          writer.addDocument(document);
        }
        writer.commit();
      }
      try (var reader = DirectoryReader.open(directory)) {
        long[] originalNorms = norms(reader);
        long[] expectedNorms = indexDiscount ? new long[] {2,2} : new long[] {3,2};
        if (!Arrays.equals(originalNorms,expectedNorms)) throw new AssertionError("index norms");
        String[] names = {"DEFAULT","supplied","zero_saturation","full_length"};
        float[][] profiles = {{1.2f,.75f},{.9f,.4f},{0f,.75f},{2.5f,1f}};
        var term = new Term("text","alpha");
        var query = new TermQuery(term);
        var searcher = new IndexSearcher(reader);
        for (int profile = 0; profile < profiles.length; profile++) {
          float k1 = profiles[profile][0], b = profiles[profile][1];
          var querySimilarity = new BM25Similarity(k1,b,!indexDiscount);
          if (querySimilarity.getDiscountOverlaps() == indexSimilarity.getDiscountOverlaps()) {
            throw new AssertionError("query flag must oppose index policy");
          }
          searcher.setSimilarity(querySimilarity);
          var stats = searcher.collectionStatistics("text");
          if (stats.docCount() != 2 || stats.sumTotalTermFreq() != 5 || searcher.count(query) != 2 ||
              reader.docFreq(term) != 2 || reader.totalTermFreq(term) != 2 ||
              !Arrays.equals(norms(reader),originalNorms)) throw new AssertionError("physical state changed");
          System.out.print("{\"profile\":\""+names[profile]+"\",\"index_discount\":"+indexDiscount+
            ",\"query_discount\":"+!indexDiscount+",\"k1_bits\":"+Float.floatToIntBits(k1)+
            ",\"b_bits\":"+Float.floatToIntBits(b)+",\"doc_count\":"+stats.docCount()+
            ",\"sum_total_term_freq\":"+stats.sumTotalTermFreq()+",\"alpha_doc_freq\":"+reader.docFreq(term)+
            ",\"alpha_total_term_freq\":"+reader.totalTermFreq(term)+",\"count\":"+searcher.count(query)+
            ",\"norms\":"+Arrays.toString(originalNorms)+",\"alpha\":[");
          var top = searcher.search(query,2);
          for (int i = 0; i < top.scoreDocs.length; i++) {
            if (i != 0) System.out.print(",");
            var hit = top.scoreDocs[i];
            int bits = Float.floatToIntBits(hit.score);
            if (Float.floatToIntBits(searcher.explain(query,hit.doc).getValue().floatValue()) != bits) {
              throw new AssertionError("explanation score differs");
            }
            System.out.print("{\"doc\":"+hit.doc+",\"score\":"+hit.score+",\"bits\":"+bits+"}");
          }
          System.out.println("]}");
        }
      }
    }
  }
  public static void main(String[] args) throws Exception {
    System.out.println("{\"lucene_version\":\""+Version.LATEST+"\"}");
    run(true);
    run(false);
  }
}
