import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.nio.file.Paths;
import com.eclipsesource.json.JsonArray;
import com.eclipsesource.json.JsonObject;
import org.apache.lucene.analysis.CharArraySet;
import org.apache.lucene.analysis.standard.StandardAnalyzer;
import org.apache.lucene.index.DirectoryReader;
import org.apache.lucene.queryparser.classic.QueryParser;
import org.apache.lucene.search.IndexSearcher;
import org.apache.lucene.search.Query;
import org.apache.lucene.search.ScoreDoc;
import org.apache.lucene.search.TopDocs;
import org.apache.lucene.search.similarities.BM25Similarity;
import org.apache.lucene.store.FSDirectory;

public class DumpNativeLuceneResults {
    public static void main(String[] args) throws Exception {
        try (DirectoryReader reader = DirectoryReader.open(FSDirectory.open(Paths.get(args[0])));
             BufferedReader input = new BufferedReader(new InputStreamReader(System.in))) {
            float scoreScale = args.length > 1 ? Float.parseFloat(args[1]) : 1f;
            if (!Float.isFinite(scoreScale) || scoreScale <= 0f) {
                throw new IllegalArgumentException("Score scale must be finite and positive");
            }
            IndexSearcher searcher = new IndexSearcher(reader);
            searcher.setQueryCache(null);
            searcher.setSimilarity(new BM25Similarity(1.2f, 0.75f));
            var statistics = searcher.collectionStatistics("text");
            System.err.println("text maxDoc=" + statistics.maxDoc() + " docCount="
                + statistics.docCount() + " sumTotalTermFreq=" + statistics.sumTotalTermFreq());
            QueryParser parser = new QueryParser("text", new StandardAnalyzer(CharArraySet.EMPTY_SET));
            String queryText;
            while ((queryText = input.readLine()) != null) {
                Query query = parser.parse(queryText);
                Query scoringQuery = scoreScale == 1f ? query
                    : new org.apache.lucene.search.BoostQuery(query, scoreScale);
                TopDocs results = searcher.search(scoringQuery, 100);
                JsonArray docs = new JsonArray();
                for (ScoreDoc doc : results.scoreDocs) {
                    String id = searcher.storedFields().document(doc.doc).get("id");
                    docs.add(new JsonObject().add("id", id).add("score", doc.score));
                }
                System.out.println(new JsonObject().add("query", queryText)
                    .add("count", searcher.count(query)).add("top100", docs));
            }
        }
    }
}
