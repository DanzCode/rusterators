use rusterators::generators::{Generator, PureGeneratorFactory, PureGenerator};


fn create_line_generator<'a>(file_content:Result<String,String>) -> Generator<'a,String,Result<(),String>,()> {
    Generator::new(move |g| {
        match &file_content {
            Ok(content) => {
                for l in content.lines() {

                    g.yield_val(String::from(l.trim()))
                }
                Ok(())
            },
            Err(e) => {
                Err(String::from("failure"))
            }
        }
    })
}

fn main() {

    let mut g=create_line_generator(Ok(String::from(r#"1 line
    2 line
    3 line
    4 line"#)));

    //g.resume(());
    for s in  &mut g {
        println!("{}",s)
    }
    println!("d {:?} {:?}",g.result(),std::env::current_dir().unwrap());
}