import com.eclipsesource.json.JsonObject;
import org.apache.lucene.search.IndexSearcher;
import org.apache.lucene.search.similarities.BM25Similarity;

/** Validated optional exact-bit boundary shared by native protocol and dump helpers. */
public final class NativeBm25Profile {
    public final boolean configured;
    public final float scoreScale;
    public final BM25Similarity similarity;

    private NativeBm25Profile(boolean configured, float scoreScale, BM25Similarity similarity) {
        this.configured = configured;
        this.scoreScale = scoreScale;
        this.similarity = similarity;
    }

    private static float scalar(String bits) {
        if (!bits.matches("[0-9a-f]{8}")) {
            throw new IllegalArgumentException("Expected eight lowercase hex digits for BM25 bits");
        }
        return Float.intBitsToFloat((int) Long.parseLong(bits, 16));
    }

    public static NativeBm25Profile parse(String[] args) {
        if (args.length != 1 && args.length != 2 && args.length != 4) {
            throw new IllegalArgumentException("Usage: INDEX [SCALE [K1_BITS B_BITS]]");
        }
        float scale = args.length > 1 ? Float.parseFloat(args[1]) : 1f;
        if (!Float.isFinite(scale) || scale <= 0f) {
            throw new IllegalArgumentException("Score scale must be finite and positive");
        }
        if (args.length != 4) {
            return new NativeBm25Profile(false, scale, null);
        }
        if (scale != 1f) {
            throw new IllegalArgumentException("Configured BM25 requires score scale 1");
        }
        float k1 = scalar(args[2]);
        float b = scalar(args[3]);
        if (!Float.isFinite(k1) || !Float.isFinite(b)) {
            throw new IllegalArgumentException("BM25 parameters must be finite");
        }
        // The actual constructor validates the effective binary32 parameter domain.
        return new NativeBm25Profile(true, scale, new BM25Similarity(k1, b));
    }

    public static String receipt(IndexSearcher searcher, float scale) throws java.io.IOException {
        if (!(searcher.getSimilarity() instanceof BM25Similarity)) {
            throw new IllegalArgumentException("Installed Similarity is not BM25");
        }
        BM25Similarity installed = (BM25Similarity) searcher.getSimilarity();
        var stats = searcher.collectionStatistics("text");
        return new JsonObject().add("protocol", "bm25-profile-v1")
            .add("engine", "lucene").add("field", "text")
            .add("k1_bits", String.format(java.util.Locale.ROOT, "%08x", Float.floatToRawIntBits(installed.getK1())))
            .add("b_bits", String.format(java.util.Locale.ROOT, "%08x", Float.floatToRawIntBits(installed.getB())))
            .add("scale_bits", String.format(java.util.Locale.ROOT, "%08x", Float.floatToRawIntBits(scale)))
            .add("collection_statistics", "physical").add("query_cache", "disabled")
            .add("doc_count", stats == null ? 0L : stats.docCount())
            .add("sum_total_term_freq", stats == null ? 0L : stats.sumTotalTermFreq()).toString();
    }
}
