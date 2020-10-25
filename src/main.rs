use crate::exchange::execution::{CoroutineInvoker, CoroutineChannel};

mod exchange;
mod utils;

fn main() {
    println!("{:?}",CoroutineInvoker::new(|mut con:&mut CoroutineChannel<u32,u32,u32>,i| {
        con.yield_with(i);
        2
    }).invoke(5).1);
}
