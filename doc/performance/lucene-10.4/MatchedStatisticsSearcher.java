import java.io.IOException;
import org.apache.lucene.index.IndexReader;
import org.apache.lucene.search.CollectionStatistics;
import org.apache.lucene.search.IndexSearcher;

/** Match Tantivy's all-document BM25 population and persisted token total. */
public final class MatchedStatisticsSearcher extends IndexSearcher {
    private final long tokenTotal;
    public MatchedStatisticsSearcher(IndexReader reader, long tokenTotal) {
        super(reader);
        this.tokenTotal = tokenTotal;
    }
    @Override
    public CollectionStatistics collectionStatistics(String field) throws IOException {
        CollectionStatistics stats = super.collectionStatistics(field);
        if (stats == null || !field.equals("text")) return stats;
        return new CollectionStatistics(field, stats.maxDoc(), stats.maxDoc(),
            tokenTotal, stats.sumDocFreq());
    }
}
