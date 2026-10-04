import java.util.*;
import org.apache.lucene.analysis.*;
import org.apache.lucene.analysis.tokenattributes.*;
import org.apache.lucene.document.*;
import org.apache.lucene.index.*;
import org.apache.lucene.search.*;
import org.apache.lucene.search.similarities.BM25Similarity;
import org.apache.lucene.store.ByteBuffersDirectory;
import org.apache.lucene.util.Version;

/** Small native IndexSearcher reference; no collection-statistics override. */
public class OverlapNormReference {
  record Tok(String term, int position, int length) {
    Tok(String term, int position) { this(term, position, 1); }
  }
  static class Tokens extends TokenStream {
    final Tok[] tokens;
    final CharTermAttribute term = addAttribute(CharTermAttribute.class);
    final PositionIncrementAttribute increment = addAttribute(PositionIncrementAttribute.class);
    final PositionLengthAttribute length = addAttribute(PositionLengthAttribute.class);
    final OffsetAttribute offsets = addAttribute(OffsetAttribute.class);
    int index;
    Tokens(Tok[] tokens) { this.tokens = tokens; }
    public void reset() { index = 0; }
    public boolean incrementToken() {
      if (index == tokens.length) return false;
      clearAttributes();
      Tok token = tokens[index];
      term.append(token.term());
      increment.setPositionIncrement(token.position() - (index == 0 ? -1 : tokens[index-1].position()));
      length.setPositionLength(token.length());
      offsets.setOffset(index, index+1);
      index++;
      return true;
    }
    public void end() { offsets.setOffset(tokens.length, tokens.length); increment.setPositionIncrement(0); }
  }
  static void run(String name, boolean discount, boolean basic, Tok[][][] docs) throws Exception {
    FieldType type = new FieldType();
    type.setTokenized(true);
    type.setIndexOptions(basic ? IndexOptions.DOCS : IndexOptions.DOCS_AND_FREQS_AND_POSITIONS);
    type.freeze();
    BM25Similarity similarity = new BM25Similarity(discount);
    try (var dir = new ByteBuffersDirectory()) {
      var config = new IndexWriterConfig().setSimilarity(similarity).setMergePolicy(NoMergePolicy.INSTANCE);
      try (var writer = new IndexWriter(dir, config)) {
        for (Tok[][] values : docs) {
          Document document = new Document();
          for (Tok[] tokens : values) document.add(new Field("text", new Tokens(tokens), type));
          writer.addDocument(document);
        }
        writer.commit();
      }
      try (var reader = DirectoryReader.open(dir)) {
        var searcher = new IndexSearcher(reader);
        searcher.setSimilarity(similarity);
        var stats = searcher.collectionStatistics("text");
        System.out.print("{\"name\":\""+name+"\",\"discount\":"+discount+",\"basic\":"+basic+
          ",\"doc_count\":"+stats.docCount()+",\"sum_total_term_freq\":"+stats.sumTotalTermFreq()+",\"norms\":[");
        var norms = reader.leaves().get(0).reader().getNormValues("text");
        for (int doc = 0; doc < reader.maxDoc(); doc++) {
          if (doc != 0) System.out.print(",");
          System.out.print(norms.advanceExact(doc) ? norms.longValue() : 0);
        }
        System.out.print("],\"alpha\":[");
        var query = new TermQuery(new Term("text", "alpha"));
        var top = searcher.search(query, reader.maxDoc());
        for (int i = 0; i < top.scoreDocs.length; i++) {
          if (i != 0) System.out.print(",");
          var hit = top.scoreDocs[i];
          System.out.print("{\"doc\":"+hit.doc+",\"score\":"+hit.score+",\"bits\":"+Float.floatToIntBits(hit.score)+"}");
        }
        System.out.println("]}");
      }
    }
  }
  public static void main(String[] args) throws Exception {
    System.out.println("{\"lucene_version\":\""+Version.LATEST+"\"}");
    Tok[][][] standard = {{{new Tok("alpha",0),new Tok("synonym",0),new Tok("beta",1)}},
                         {{new Tok("alpha",0),new Tok("beta",1)}}};
    run("default", true, false, standard);
    run("count_all", false, false, standard);
    run("basic", true, true, standard);
    run("same_term", true, false, new Tok[][][] {
      {{new Tok("alpha",0),new Tok("alpha",0),new Tok("beta",1)}},standard[1]});
    run("multi_value", true, false, new Tok[][][] {
      {{new Tok("alpha",0),new Tok("synonym",0)},{new Tok("beta",0)}},standard[1]});
    run("position_length", true, false, new Tok[][][] {
      {{new Tok("alpha",0,3),new Tok("synonym",0),new Tok("beta",1)}},standard[1]});
    run("start_gap", true, false, new Tok[][][] {
      {{new Tok("alpha",5),new Tok("synonym",5),new Tok("beta",8)}},standard[1]});
  }
}
