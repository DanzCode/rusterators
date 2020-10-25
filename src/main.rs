use crate::coroutines::execution::{CoroutineInvoker, CoroutineChannel};
use crate::generators::ReceivingGenerator;

mod coroutines;
mod utils;
mod generators;

fn main() {
    /*println!("{:?}",CoroutineInvoker::new(Box::new(|mut con:&mut CoroutineChannel<u32,u32,u32>,i| {
        con.yield_with(i);

        2
    })).invoke(5).1);
     */
    let mut generator = ReceivingGenerator::new(|g, i| {
        let i: i32 = g.yield_val(3);

        2
    });
    for t in generator.build_iterator(|| 3) {
        println!("{}",t)
    }
    println!("{:?}",generator.generator_result())
}
