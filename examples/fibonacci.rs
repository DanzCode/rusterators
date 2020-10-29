extern crate rusterators;
use rusterators::generators::{BoostedGenerator, MonoGenerator, GeneratorChannel};

fn main() {
    for f in MonoGenerator::new(|g| {
        let mut current=(0,1);
        loop {
            g.yield_val(current.0);
            current=(current.1, current.0+current.1);
        }
    }).into_iter().take(42) {
      println!("{}",f)
    }

}