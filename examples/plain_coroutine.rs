use rusterators::coroutines::{Coroutine, CoroutineFactory, DynCoroutineFactory};
fn main() {
    let coroutine=DynCoroutineFactory::new(|mut chan,mut i:i32| {
        let mut counter=0;
        while i!=10 {
            counter+=1;
            i=chan.suspend(i.cmp(&10));
        }
        counter
    });
    let mut  co=coroutine.build();
    for i in 5..11 {
        println!("{:?}",co.resume(i));
    }
}