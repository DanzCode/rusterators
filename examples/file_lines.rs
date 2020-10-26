use rusterators::generators::{Generator, PureGeneratorFactory, PureGenerator};


fn create_line_generator<'a>(file_content:Result<&'static str,String>) -> Generator<'a,String,Result<(),String>,()> {
    println!("{:?}",file_content);
    let content=match file_content {
        Ok(o) => Some(String::from(o)),
        Err(e) => None
    };
    Generator::new(move |g| {
        println!("gen closure invoked");
        match &content {
            Some(content) => {
                println!("o {}",content);
                for l in content.lines() {
                    println!("d{}",l);
                    g.yield_val(String::from(l))
                }
                println!("d2");
                Ok(())
            },
            None => {
                println!("e");
                Err(String::from("failure"))
            }
        }
    })
}

fn main() {

    let mut g=create_line_generator(Ok(r#"1 line
    2 line
    3 line
    4 line"#));
    println!("d");
    g.resume(());
    /*for s in  &mut g {
        println!("{}",s)
    }*/
    //println!("d {:?} {:?}",g.result(),std::env::current_dir().unwrap());
}