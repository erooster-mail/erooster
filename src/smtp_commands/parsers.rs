use nom::{
    bytes::complete::take_while1,
    character::complete::char,
    error::{context, VerboseError},
    multi::many0,
    sequence::delimited,
    IResult,
};

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

// TODO parse relay vs no relay
pub fn localpart_arguments(input: &str) -> Res<Vec<&str>> {
    println!("localpart_arguments Input: {}", input);
    context(
        "localpart_arguments",
        delimited(char('<'), many0(take_while1(|c: char| c != ',')), char('>')),
    )(input)
    .map(|(x, y)| (x, y))
}
