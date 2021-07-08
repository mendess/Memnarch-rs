use crate::util::tuple_map::TupleMap;
use chrono::{Duration, NaiveTime};
use lazy_static::lazy_static;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{self as character, space0 as spc},
    combinator::{map, opt},
    regexp::str::re_find,
    sequence::{delimited, preceded, terminated, tuple},
    Finish, IResult,
};
use regex::Regex;

fn spaced<'s, P, O, E>(p: P) -> impl FnMut(&'s str) -> IResult<&str, O, E>
where
    P: nom::Parser<&'s str, O, E>,
    E: nom::error::ParseError<&'s str>,
{
    delimited(spc, p, spc)
}

fn all(s: &str) -> IResult<&str, &str> {
    Ok(("", s))
}

fn parse_number(s: &str) -> IResult<&str, i16> {
    map(character::digit1, |s: &str| s.parse::<i16>().unwrap())(s)
}

fn date(input: &str) -> IResult<&str, PartialDate> {
    let mut slash_num = opt(preceded(tag("/"), parse_number));

    let (input, day) = parse_number(input)?.map_snd(|o| o as u32);
    let (input, month) = slash_num(input)?.map_snd(|o| o.map(|i| i as u32));
    let (input, year) = slash_num(input)?.map_snd(|o| o.map(|i| i as i32));

    Ok((input, PartialDate { year, month, day }))
}

fn time(input: &str) -> IResult<&str, NaiveTime> {
    let mut colon_time = opt(preceded(tag(":"), parse_number));

    let (input, hour) = parse_number(input)?;
    let (input, min) = colon_time(input)?.map_snd(Option::unwrap_or_default);
    let (input, sec) = colon_time(input)?.map_snd(Option::unwrap_or_default);
    Ok((input, NaiveTime::from_hms(hour as _, min as _, sec as _)))
}

fn duration(input: &str) -> IResult<&str, Duration> {
    lazy_static! {
        static ref SECONDS: Regex = Regex::new("^(s|sec|secs|seconds?|segundos?) ").unwrap();
        static ref MINUTES: Regex = Regex::new("^(m|min|mins|minutes?|minutos?) ").unwrap();
        static ref HOURS: Regex = Regex::new("^(h|hours?|horas?) ").unwrap();
        static ref DAYS: Regex = Regex::new("^(d|days?|dias?) ").unwrap();
        static ref WEEKS: Regex = Regex::new("^(w|weeks?|semanas?) ").unwrap();
        static ref MONTHS: Regex = Regex::new("^(months?|mes(es)?) ").unwrap();
        static ref YEARS: Regex = Regex::new("^(y|years?|anos?) ").unwrap();
    };
    let re = |r: &Regex| re_find(r.clone());

    let (input, amt) = terminated(parse_number, spc)(input)?.map_snd(|o| o as i64);
    let (input, dur) = alt((
        map(re(&*SECONDS), |_| Duration::seconds(amt)),
        map(re(&*MINUTES), |_| Duration::minutes(amt)),
        map(re(&*HOURS), |_| Duration::hours(amt)),
        map(re(&*DAYS), |_| Duration::days(amt)),
        map(re(&*WEEKS), |_| Duration::weeks(amt)),
        map(re(&*MONTHS), |_| Duration::days(30 * amt)),
        map(re(&*YEARS), |_| Duration::days(365 * amt)),
    ))(input)?;
    Ok((input, dur))
}

fn at_time(input: &str) -> IResult<&str, (NaiveTime, &str)> {
    let (input, (time, text)) = tuple((preceded(tag("at"), spaced(time)), all))(input)?;

    Ok((input, (time, text)))
}

fn on_day(input: &str) -> IResult<&str, ((PartialDate, NaiveTime), &str)> {
    let (input, (date, time, text)) = tuple((
        delimited(tag("day"), spaced(date), tag("at")),
        spaced(time),
        all,
    ))(input)?;

    Ok((input, ((date, time), text)))
}

fn in_time(input: &str) -> IResult<&str, (Duration, &str)> {
    tuple((duration, all))(input)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartialDate {
    pub day: u32,
    pub month: Option<u32>,
    pub year: Option<i32>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimeSpec {
    Duration(Duration),
    Time(NaiveTime),
    Date((PartialDate, NaiveTime)),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reminder<'text> {
    pub text: &'text str,
    pub when: TimeSpec,
}

// at 08:32 arbitrarytext
// day 03 at 08:30 arbitrarytext
// day 03/08 at 8 arbitrarytext
// 4h arbitrarytext
// 4 h arbitrarytext
// 4 hours arbitrarytext

pub fn parse(a: &str) -> Result<Reminder<'_>, nom::error::Error<&str>> {
    alt((
        map(at_time, |(d, text)| Reminder {
            text,
            when: TimeSpec::Time(d),
        }),
        map(on_day, |(d, text)| Reminder {
            text,
            when: TimeSpec::Date(d),
        }),
        map(in_time, |(d, text)| Reminder {
            text,
            when: TimeSpec::Duration(d),
        }),
    ))(a)
    .finish()
    .map(|(_, r)| r)
}

#[cfg(test)]
mod test {
    use super::{parse, Reminder, TimeSpec};
    use chrono::Duration;

    macro_rules! make_test {
        ($($time:ident => $ctor:ident$(* $mult:expr)?),* $(,)?) => {
            paste::paste! {$(
                #[test]
                fn [<$ctor _from_ $time _no_space>]() {
                    let r = Reminder {
                        text: "cenas",
                        when: TimeSpec::Duration(Duration::$ctor(2 $(* $mult)?))
                    };
                    let x = concat!("2", stringify!($time), " cenas");
                    eprintln!("{:?}", x);
                    assert_eq!(
                        parse(x).unwrap(),
                        r,
                    );
                    assert_eq!(
                        parse(concat!("2", stringify!($time), " cenas")).unwrap(),
                        r
                    );
                }

                #[test]
                fn [<$ctor _from_ $time _space>]() {
                    let r = Reminder {
                        text: "cenas",
                        when: TimeSpec::Duration(Duration::$ctor(2 $(* $mult)?))
                    };
                    assert_eq!(
                        parse(concat!("2 ", stringify!($time), " cenas")).unwrap(),
                        r,
                    );
                    assert_eq!(
                        parse(concat!("2 ", stringify!($time), " cenas")).unwrap(),
                        r,
                    );
                }
            )*}
        }
    }

    make_test! {
        s => seconds,
        sec => seconds,
        secs => seconds,
        second => seconds,
        seconds => seconds,
        segundo => seconds,
        segundos => seconds,
        m => minutes,
        min => minutes,
        minute => minutes,
        minutes => minutes,
        minuto => minutes,
        minutos => minutes,
        h => hours,
        hour => hours,
        hours => hours,
        hora => hours,
        horas => hours,
        d => days,
        day => days,
        days => days,
        dia => days,
        dias => days,
        w => weeks,
        week => weeks,
        weeks => weeks,
        semana => weeks,
        semanas => weeks,
        month => days * 30,
        months => days * 30,
        mes => days * 30,
        meses => days * 30,
        year => days * 365,
        years => days * 365,
        ano => days * 365,
        anos => days * 365,
    }
}
