// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use {
    nom::{
        bytes::complete::take_while1, character::complete::char, error::context, multi::many0,
        sequence::delimited, IResult, Parser,
    },
    nom_language::error::VerboseError,
    tracing::instrument,
};

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

// TODO parse relay vs no relay
#[instrument(skip(input))]
fn localpart(input: &str) -> Res<'_, Vec<&str>> {
    context(
        "localpart",
        many0(take_while1(|c: char| c != ',' && c != '>')),
    )
    .parse(input)
}

#[instrument(skip(input))]
pub fn localpart_arguments(input: &str) -> Res<'_, Vec<&str>> {
    context(
        "localpart_arguments",
        delimited(char('<'), localpart, char('>')),
    )
    .parse(input)
}
