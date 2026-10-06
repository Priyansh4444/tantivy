"""Ensure corrupted runtime results cannot be admitted to the benchmark."""
import copy,unittest
from balanced import validate_dump,native_run

CASE={'id':0}
DUMP={'id':0,'count':2,'hits':[[0,0x3f800000],[1,0x3f000000]],'reported':{'value':2,'relation':'eq'},'oracle':{}}
class GateTests(unittest.TestCase):
    def test_order_length_bits_and_relations_are_checked(self):
        validate_dump(CASE,DUMP)
        variants=[]
        d=copy.deepcopy(DUMP);d['hits'].reverse();variants.append(d)
        d=copy.deepcopy(DUMP);d['hits'][1][0]=0;variants.append(d)
        d=copy.deepcopy(DUMP);d['hits'][0][1]=0x7fc00000;variants.append(d)
        d=copy.deepcopy(DUMP);d['count']=1;variants.append(d)
        d=copy.deepcopy(DUMP);d['reported']['value']=3;variants.append(d)
        d=copy.deepcopy(DUMP);d['reported']['relation']='unknown';variants.append(d)
        for d in variants:
            with self.subTest(dump=d),self.assertRaises(RuntimeError):validate_dump(CASE,d)
    def test_valid_lower_bound_does_not_have_to_equal_exact_count(self):
        d=copy.deepcopy(DUMP);d['count']=20;d['hits']=[[i,0x3f800000] for i in range(10)];d['reported']={'value':12,'relation':'gte'}
        validate_dump(CASE,d)
        d['reported']['value']=21
        with self.assertRaises(RuntimeError):validate_dump(CASE,d)
    def test_each_timed_batch_is_tied_to_verified_results(self):
        class FakeWorker:
            engine='tantivy';expected={0:DUMP}
            checksum=4
            def request(self,request):return {'id':0,'mode':'count','ns':[100,100],'checksum':self.checksum}
        worker=FakeWorker();native_run(worker,CASE,'count',2)
        worker.checksum=2
        with self.assertRaises(RuntimeError):native_run(worker,CASE,'count',2)
if __name__=='__main__':unittest.main()
