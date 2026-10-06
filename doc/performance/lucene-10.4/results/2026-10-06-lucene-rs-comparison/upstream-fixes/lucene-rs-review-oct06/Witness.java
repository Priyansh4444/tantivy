import org.apache.lucene.analysis.standard.StandardAnalyzer;
import org.apache.lucene.analysis.tokenattributes.CharTermAttribute;
import org.apache.lucene.analysis.tokenattributes.PositionIncrementAttribute;
import org.apache.lucene.search.similarities.BM25Similarity;
import org.apache.lucene.search.CollectionStatistics;
import org.apache.lucene.search.TermStatistics;
import org.apache.lucene.util.BytesRef;
public class Witness {
  public static void main(String[] args) throws Exception {
    long n = 54505;
    var b = new BM25Similarity();
    var c = new CollectionStatistics("body", n,n,n,n);
    var t = new TermStatistics(new BytesRef("x"), n,n);
    float idf = b.idfExplain(c,t).getValue().floatValue();
    float score = b.scorer(1f,c,t).score(1f,1L);
    System.out.printf("idf=%08x score=%08x%n",Float.floatToRawIntBits(idf),Float.floatToRawIntBits(score));
    try(var a = new StandardAnalyzer()) {
      a.setMaxTokenLength(3);
      try(var s = a.tokenStream("body","abcdef z 😀")) {
        var text = s.addAttribute(CharTermAttribute.class);
        var inc = s.addAttribute(PositionIncrementAttribute.class);
        int pos = -1;
        s.reset();
        while(s.incrementToken()) {pos += inc.getPositionIncrement();System.out.printf("token=%s pos=%d%n",text,pos);}
        s.end();
      }
    }
  }
}
