from pathlib import Path
import json,shutil,subprocess,hashlib,os
base=Path('/home/pronsh/Coding/playground/search')
wt=base/'tantivy-pruning-counters-oct05'
out=base/'bench/native-profile-pruning-counters-oct05'
out.mkdir()
names=['leaf_score','actual_term_score','decode_for','decode_dense','decode_tail','complete_global_loaded','complete_global_unloaded','stored_bound','loaded_reduce','cache_hit','unsafe_global','route_single','route_union','route_intersection','route_sparse_dense','skip_single','skip_union','skip_intersection','route_two_term']
module='use std::sync::atomic::{AtomicU64,Ordering};\n'
module+='pub static COUNTERS:[AtomicU64;'+str(len(names))+']=[const{AtomicU64::new(0)};'+str(len(names))+'];\n'
module+='pub fn hit(i:usize){COUNTERS[i].fetch_add(1,Ordering::Relaxed);}\n'
module+='pub fn reset(){for c in &COUNTERS{c.store(0,Ordering::Relaxed);}}\n'
module+='pub fn snapshot()->Vec<u64>{COUNTERS.iter().map(|c|c.load(Ordering::Relaxed)).collect()}\n'
(wt/'src/pruning_counters.rs').write_text(module)
p=wt/'src/lib.rs';p.write_text(p.read_text()+'\n#[doc(hidden)] pub mod pruning_counters;\n')
def replace(rel,old,new):
 p=wt/rel;s=p.read_text();assert s.count(old)==1,(rel,old,s.count(old));p.write_text(s.replace(old,new))
replace('src/query/bm25.rs','pub fn score(&self, fieldnorm_id: u8, term_freq: u32) -> Score {','pub fn score(&self, fieldnorm_id: u8, term_freq: u32) -> Score {\n crate::pruning_counters::hit(0);')
replace('src/query/term_query/term_scorer.rs','fn score(&mut self) -> Score {','fn score(&mut self) -> Score {\n crate::pruning_counters::hit(1);')
r='src/postings/block_segment_postings.rs'
replace(r,'if !bm25_weight.has_safe_score_bounds() {','if !bm25_weight.has_safe_score_bounds() {\n crate::pruning_counters::hit(10);')
replace(r,'if !use_stored_max && self.skip_reader.last_doc_in_block() != TERMINATED {','if !use_stored_max && self.skip_reader.last_doc_in_block() != TERMINATED {\n crate::pruning_counters::hit(if self.block_is_loaded(){5}else{6});')
replace(r,'if let Some(score) = self.block_max_score_cache {','if let Some(score) = self.block_max_score_cache {\n crate::pruning_counters::hit(9);')
replace(r,'if let Some(skip_reader_max_score) = self.skip_reader.block_max_score(bm25_weight) {','if let Some(skip_reader_max_score) = self.skip_reader.block_max_score(bm25_weight) {\n crate::pruning_counters::hit(7);')
replace(r,'if self.block_is_loaded() {\n            let docs =','if self.block_is_loaded() {\n crate::pruning_counters::hit(8);\n            let docs =')
replace(r,'decode_bitpacked_block(\n                    &mut self.doc_decoder,','crate::pruning_counters::hit(2);\n                decode_bitpacked_block(\n                    &mut self.doc_decoder,')
replace(r,'decode_dense_block(\n                    &mut self.doc_decoder,','crate::pruning_counters::hit(3);\n                decode_dense_block(\n                    &mut self.doc_decoder,')
replace(r,'decode_vint_block(\n                    &mut self.doc_decoder,','crate::pruning_counters::hit(4);\n                decode_vint_block(\n                    &mut self.doc_decoder,')
r='src/query/boolean_query/block_wand_union.rs'
for name,idx in [('two_term_or_maxscore',18),('block_wand',12),('block_wand_single_scorer',11)]:
 p=wt/r;s=p.read_text();start=s.index('fn '+name+'(');body=s.index(') {',start)+3;s=s[:body]+f'\n crate::pruning_counters::hit({idx});'+s[body:];p.write_text(s)
replace(r,'while scorer.block_max_score() <= threshold {','while scorer.block_max_score() <= threshold {\n crate::pruning_counters::hit(15);')
replace(r,'if upper_bound.score(block_max_score_sum) <= threshold {','if upper_bound.score(block_max_score_sum) <= threshold {\n crate::pruning_counters::hit(16);')
r='src/query/boolean_query/block_wand_intersection.rs'
for name,idx in [('sparse_dense_intersection',14),('block_wand_intersection',13)]:
 p=wt/r;s=p.read_text();start=s.index('fn '+name+'(');body=s.index(') {',start)+3;s=s[:body]+f'\n crate::pruning_counters::hit({idx});'+s[body:];p.write_text(s)
replace(r,'if upper_bound.score(f64::from(leader_block_max) + secondary_block_max_sum) <= threshold {','if upper_bound.score(f64::from(leader_block_max) + secondary_block_max_sum) <= threshold {\n crate::pruning_counters::hit(17);')
crate=out/'adapter';(crate/'src/bin/shared').mkdir(parents=True)
src=wt/'doc/performance/lucene-10.4'
shutil.copy2(src/'shared/bm25_profile.rs',crate/'src/bin/shared/bm25_profile.rs')
q=(src/'do_query.rs').read_text().replace('let query = query_parser.parse_query(fields[1])?;','tantivy::pruning_counters::reset();\n        let query = query_parser.parse_query(fields[1])?;').replace('println!("{}", count);','eprintln!("COUNTERS\\t{}\\t{}\\t{:?}",command,fields[1],tantivy::pruning_counters::snapshot());\n        println!("{}", count);')
(crate/'src/bin/do_query.rs').write_text(q)
(crate/'Cargo.toml').write_text('[package]\nname="wiki-pruning-counters"\nversion="0.1.0"\nedition="2021"\n[workspace]\n[dependencies]\ntantivy={path="'+str(wt)+'"}\n[profile.release]\nlto=true\nopt-level=3\noverflow-checks=false\n')
patch=subprocess.check_output(['git','-C',str(wt),'diff'],text=True);(out/'core-instrumentation.patch').write_text(patch)
(out/'counter-map.json').write_text(json.dumps(names,indent=2)+'\n')
(out/'instrumentation-source-sha256.json').write_text(json.dumps({str(p.relative_to(base)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [wt/'src/pruning_counters.rs',crate/'src/bin/do_query.rs',crate/'src/bin/shared/bm25_profile.rs',crate/'Cargo.toml']},indent=2)+'\n')
argv=['cargo','build','--manifest-path',str(crate/'Cargo.toml'),'--target-dir',str(base/'bench/native-perf-adapter-oct03/target'),'--release','--jobs','4','--bin','do_query']
(out/'build-command.json').write_text(json.dumps(argv)+'\n')
env=os.environ.copy();env['RUSTFLAGS']='-C target-cpu=native'
with (out/'build.log').open('w') as f:subprocess.run(argv,env=env,stdout=f,stderr=subprocess.STDOUT,check=True)
shutil.copy2(base/'bench/native-perf-adapter-oct03/target/release/do_query',out/'do_query-counters')
print('instrumented untimed artifact ready')
