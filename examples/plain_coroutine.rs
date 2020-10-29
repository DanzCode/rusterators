use rusterators::coroutines::{Coroutine};
fn main() {
    let mut coroutine= Coroutine::new(|mut chan, mut i:i32| {
        let mut counter=0;
        while i!=10 {
            counter+=1;
            i=chan.suspend(i.cmp(&10));
        }
        counter
    });

    for i in 5..11 {
        println!("{:?}",coroutine.resume(i));
    }
}