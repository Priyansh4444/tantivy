import java.nio.file.Files;
import java.nio.file.Path;
import org.apache.lucene.analysis.core.WhitespaceAnalyzer;
import org.apache.lucene.document.Document;
import org.apache.lucene.document.TextField;
import org.apache.lucene.document.Field;
import org.apache.lucene.index.DirectoryReader;
import org.apache.lucene.index.IndexWriter;
import org.apache.lucene.index.IndexWriterConfig;
import org.apache.lucene.index.Term;
import org.apache.lucene.search.CollectionStatistics;
import org.apache.lucene.search.IndexSearcher;
import org.apache.lucene.search.TermQuery;
import org.apache.lucene.search.TermStatistics;
import org.apache.lucene.search.similarities.BM25Similarity;
import org.apache.lucene.util.BytesRef;
import org.apache.lucene.store.ByteBuffersDirectory;

public class Bm25ParametersReference {
  static final float[][] PROFILES = {
    {1.2f, .75f}, {.9f, .4f}, {0f, .75f}, {-0f, .75f},
    {.1f, 0f}, {2.5f, 1f}, {Float.MIN_VALUE, 1f}, {Float.MAX_VALUE, 1f},
    {1.2f, -0f}, {0f, 1f}
  };
  static int bits(float value) { return Float.floatToIntBits(value); }
  static class AverageOverride extends BM25Similarity {
    final float average;
    AverageOverride(float k1, float b, float average) { super(k1,b); this.average=average; }
    @Override protected float avgFieldLength(CollectionStatistics ignored) { return average; }
  }
  public static void main(String[] args) throws Exception {
    var output = new StringBuilder();
    for (float k1 : new float[] {-1f, Float.NaN, Float.POSITIVE_INFINITY,
        Float.NEGATIVE_INFINITY, -Float.MIN_VALUE, 0f, -0f, Float.MIN_VALUE, 1.2f, Float.MAX_VALUE}) {
      boolean valid;
      try { var model=new BM25Similarity(k1,.75f); valid=true;
        if (Float.floatToRawIntBits(model.getK1()) != Float.floatToRawIntBits(k1)) throw new AssertionError();
      } catch (IllegalArgumentException rejected) { valid=false; }
      output.append(String.format("k1,%08x,%s%n",bits(k1),valid));
    }
    for (float b : new float[] {-Float.MIN_VALUE, Float.NaN, Float.POSITIVE_INFINITY,
        Float.NEGATIVE_INFINITY, Math.nextUp(1f), 0f, -0f, Float.MIN_VALUE, .75f, 1f}) {
      boolean valid;
      try { var model=new BM25Similarity(1.2f,b); valid=true;
        if (Float.floatToRawIntBits(model.getB()) != Float.floatToRawIntBits(b)) throw new AssertionError();
      } catch (IllegalArgumentException rejected) { valid=false; }
      output.append(String.format("b,%08x,%s%n",bits(b),valid));
    }
    try (var dir=new ByteBuffersDirectory();
         var writer=new IndexWriter(dir,new IndexWriterConfig(new WhitespaceAnalyzer()))) {
      for (String text : new String[] {"alpha alpha beta","beta","alpha beta"}) {
        var document=new Document(); document.add(new TextField("text",text,Field.Store.NO));
        writer.addDocument(document);
      }
      writer.commit();
      try (var reader=DirectoryReader.open(writer)) {
        var searcher=new IndexSearcher(reader);
        var query=new TermQuery(new Term("text","alpha"));
        for (var profile : PROFILES) {
          searcher.setSimilarity(new BM25Similarity(profile[0],profile[1]));
          for (int doc : new int[] {0,2}) {
            float score=searcher.explain(query,doc).getValue().floatValue();
            output.append(String.format("term,%08x,%08x,%d,%08x%n",bits(profile[0]),bits(profile[1]),doc,bits(score)));
          }
        }
      }
    }
    var stats=new CollectionStatistics("text",347,347,34700,34700);
    var term=new TermStatistics(new BytesRef("alpha"),53,400);
    float[] frequencies={0f, .5f, 1f, 7f, 254f, 255f, 256f, (float)0xffffffffL};
    float[] boosts={0f,-0f,1f,3.25f,-2f,Float.MAX_VALUE};
    for (var profile : PROFILES) for (float average : new float[] {1f,100f,10000f}) {
      long hash=0xcbf29ce484222325L;
      for (float boost : boosts) {
        var scorer=new AverageOverride(profile[0],profile[1],average).scorer(boost,stats,term);
        for (int norm=0;norm<256;norm++) for (float frequency : frequencies) {
          hash ^= Integer.toUnsignedLong(bits(scorer.score(frequency,norm)));
          hash *= 0x100000001b3L;
        }
      }
      output.append(String.format("matrix,%08x,%08x,%08x,%016x%n",bits(profile[0]),bits(profile[1]),bits(average),hash));
    }
    // Separate subclass-only averages, not valid native physical index statistics.
    for (float average : new float[] {0f,Float.MIN_VALUE}) for (float k1 : new float[] {0f,-0f,1.2f}) {
      var scorer=new AverageOverride(k1,1f,average).scorer(1f,stats,term);
      for (int norm : new int[] {0,1,255})
        output.append(String.format("exceptional,%08x,%08x,%d,%08x%n",bits(k1),bits(average),norm,bits(scorer.score(1f,norm))));
    }
    Files.writeString(Path.of(args[0]),output.toString());
    System.out.print(output);
  }
}
