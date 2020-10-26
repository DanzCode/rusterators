use rusterators::generators::{Generator, PureGeneratorFactory, PureGenerator};


fn create_line_generator<'a>(file_content:Result<String,String>) -> PureGenerator<'a,String,Result<(),String>> {
    Generator::new(move |g| {
        match &file_content {
            Ok(content) => {
                g.yield_all(content.lines().map(|s| String::from(s.trim())));
                Ok(())
            },
            Err(e) => {
                Err(String::from("failed to read lines"))
            }
        }
    })
}

fn main() {

    let mut g=create_line_generator(Ok(String::from(r#"1 line
    2 line
    3 line
    4 line"#)));

    for s in  &mut g {
        println!("{}",s)
    }

    println!("result: {:?}",g.result());


    let mut g = create_line_generator(Err("".into()));
    for s in &mut g {
        println!("never read: {}", s);
    }
    println!("result: {:?}",g.result());

}