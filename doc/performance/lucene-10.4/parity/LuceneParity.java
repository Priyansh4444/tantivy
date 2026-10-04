import java.io.*;
import java.util.*;
import com.eclipsesource.json.*;
import org.apache.lucene.analysis.*;
import org.apache.lucene.analysis.core.WhitespaceAnalyzer;
import org.apache.lucene.analysis.tokenattributes.*;
import org.apache.lucene.document.*;
import org.apache.lucene.index.*;
import org.apache.lucene.search.*;
import org.apache.lucene.search.similarities.BM25Similarity;
import org.apache.lucene.store.ByteBuffersDirectory;

public class LuceneParity {
    static class Tokens extends TokenStream {
        final JsonArray words;
        final CharTermAttribute term = addAttribute(CharTermAttribute.class);
        final PositionIncrementAttribute increment = addAttribute(PositionIncrementAttribute.class);
        final OffsetAttribute offset = addAttribute(OffsetAttribute.class);
        int index, nextOffset;
        Tokens(JsonArray words) { this.words = words; }
        public void reset() { index = 0; nextOffset = 0; }
        public boolean incrementToken() {
            if (index == words.size()) return false;
            clearAttributes();
            String word = words.get(index++).asString();
            term.append(word); increment.setPositionIncrement(1);
            offset.setOffset(nextOffset, nextOffset + word.length());
            nextOffset += word.length() + 1;
            return true;
        }
        public void end() { offset.setOffset(Math.max(0,nextOffset-1),Math.max(0,nextOffset-1)); }
    }
    static Query queryFromAst(JsonObject ast) {
        switch (ast.getString("type",null)) {
            case "term": return new TermQuery(new Term("text",ast.getString("term",null)));
            case "boost": return new BoostQuery(queryFromAst(ast.get("query").asObject()),ast.get("boost").asFloat());
            case "phrase": {
                var builder = new PhraseQuery.Builder(); builder.setSlop(ast.getInt("slop",0));
                int position=0;
                for(var term : ast.get("terms").asArray()) builder.add(new Term("text",term.asString()),position++);
                return builder.build();
            }
            case "bool": {
                var builder = new BooleanQuery.Builder();
                for(var value : ast.get("clauses").asArray()) {
                    var clause = value.asObject();
                    BooleanClause.Occur occur = switch(clause.getString("occur",null)) {
                        case "must" -> BooleanClause.Occur.MUST;
                        case "should" -> BooleanClause.Occur.SHOULD;
                        case "must_not" -> BooleanClause.Occur.MUST_NOT;
                        case "filter" -> BooleanClause.Occur.FILTER;
                        default -> throw new IllegalArgumentException("unsupported occurrence");
                    };
                    builder.add(queryFromAst(clause.get("query").asObject()),occur);
                }
                builder.setMinimumNumberShouldMatch(ast.getInt("minimum_should_match",0));
                return builder.build();
            }
            default: throw new IllegalArgumentException("unsupported query type");
        }
    }
    static JsonArray hits(IndexSearcher searcher, List<ScoreDoc> scored) throws Exception {
        scored.sort((a,b)-> { int score=Float.compare(b.score,a.score); return score!=0?score:Integer.compare(a.doc,b.doc); });
        var output = new JsonArray();
        for (var hit : scored) output.add(new JsonObject().add("id",Long.parseLong(searcher.storedFields().document(hit.doc).get("id"))).add("score",hit.score));
        return output;
    }

    static JsonObject run(JsonObject input) throws Exception {
        var type = new FieldType();
        type.setTokenized(true); type.setOmitNorms(!input.getBoolean("fieldnorms",true)); type.setIndexOptions(IndexOptions.DOCS_AND_FREQS_AND_POSITIONS); type.freeze();
        try (var directory = new ByteBuffersDirectory()) {
            var config = new IndexWriterConfig(new WhitespaceAnalyzer());
            config.setMergePolicy(NoMergePolicy.INSTANCE); config.setSimilarity(new BM25Similarity(1.2f,0.75f));
            try (var writer = new IndexWriter(directory,config)) {
                int segment = -1;
                for (var value : input.get("documents").asArray()) {
                    var document = value.asObject();
                    int nextSegment = document.getInt("segment",0);
                    if (segment != -1 && segment != nextSegment) writer.commit();
                    segment = nextSegment;
                    var output = new Document();
                    output.add(new StringField("id", document.get("id").toString(),Field.Store.YES));
                    var words = document.get("tokens");
                    if (words != null && !words.isNull()) output.add(new Field("text",new Tokens(words.asArray()),type));
                    writer.addDocument(output);
                }
                writer.commit();
                for (var value : input.get("documents").asArray()) {
                    var document = value.asObject();
                    if (document.getBoolean("deleted",false)) writer.deleteDocuments(new Term("id",document.get("id").toString()));
                }
                writer.commit();
                if(input.getBoolean("merge",false)) {
                    writer.getConfig().setMergePolicy(new LogByteSizeMergePolicy());
                    writer.forceMerge(1); writer.commit();
                }
                try (var reader = DirectoryReader.open(writer)) {
                    var searcher = new IndexSearcher(reader);
                    searcher.setSimilarity(new BM25Similarity(1.2f,0.75f)); searcher.setQueryCache(null);
                    var stats = searcher.collectionStatistics("text");
                    var queries = new JsonArray();
                    for (var value : input.get("queries").asArray()) {
                        var definition = value.asObject();
                        Query query = queryFromAst(definition.get("query").asObject());
                        var top = searcher.search(query,10);
                        var scored = new ArrayList<ScoreDoc>();
                        var weight = searcher.createWeight(searcher.rewrite(query),ScoreMode.COMPLETE,1f);
                        for (var leaf : reader.leaves()) {
                            var scorer = weight.scorer(leaf);
                            if (scorer==null) continue;
                            var twoPhase = scorer.twoPhaseIterator();
                            var iterator = twoPhase == null ? scorer.iterator() : TwoPhaseIterator.asDocIdSetIterator(twoPhase);
                            var live = leaf.reader().getLiveDocs();
                            for(int doc=iterator.nextDoc();doc!=DocIdSetIterator.NO_MORE_DOCS;doc=iterator.nextDoc()) {
                                if(live==null || live.get(doc)) scored.add(new ScoreDoc(leaf.docBase+doc,scorer.score()));
                            }
                        }
                        queries.add(new JsonObject().add("name",definition.get("name")).add("count",searcher.count(query)).add("exhaustive",hits(searcher,scored)).add("top",hits(searcher,new ArrayList<>(Arrays.asList(top.scoreDocs)))));
                    }
                    return new JsonObject().add("name",input.get("name")).add("statistics",new JsonObject().add("max_doc",reader.maxDoc()).add("live_docs",reader.numDocs()).add("field_doc_count",stats==null?0:stats.docCount()).add("total_tokens",stats==null?0:stats.sumTotalTermFreq()).add("segments",reader.leaves().size())).add("queries",queries);
                }
            }
        }
    }
    public static void main(String[] args) throws Exception {
        var input = new BufferedReader(new InputStreamReader(System.in));
        String line;
        while ((line=input.readLine())!=null) System.out.println(run(Json.parse(line).asObject()));
    }
}
