use crate::coroutines::execution::{CoroutineFactory, CoroutineChannel};
use crate::generators::{ReceivingGeneratorFactory, PureGeneratorFactory, Generator};


mod coroutines;
mod utils;
mod generators;

fn main() {
     //let mut coroutine = CoroutineFactory::new(|mut con: &mut CoroutineChannel<u32, u32, u32>, i| {
       //  con.suspend(i);

         //2
     //}).build();
    // let iter=coroutine.iter();

    let mut generator = Generator::new(|chan| {
        for t in 0..10 {
             chan.yield_val(t);

        }
        chan.yield_all(3..5);
        chan.yield_from(Generator::new(|b| {
            b.yield_all((0..10).map(|x| x+34));
            34
        })).unwrap()
    });
  // let iter=generator.iter1();
    for i in &mut generator {
        println!("{}",i)
    }
    println!("{} {:?}", generator.has_completed(), generator.result())
}

fn test(mut p0: Generator<i32, (), ()>) {

}