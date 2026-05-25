use std::time::Duration;

const MAX_TIMEOUT_SECONDS: u64 = 28 * 24 * 60 * 60;

pub fn parse_timeout_duration(input: &str) -> Result<Option<Duration>, &'static str> {
    let input = input.trim().to_ascii_lowercase();
    if input == "clear" || input == "0" {
        return Ok(None);
    }
    let (number, multiplier) = match input.as_bytes().last() {
        Some(b'm') => (&input[..input.len() - 1], 60),
        Some(b'h') => (&input[..input.len() - 1], 60 * 60),
        Some(b'd') => (&input[..input.len() - 1], 24 * 60 * 60),
        _ => return Err("Duration must be `10m`, `2h`, `1d`, `0`, or `clear`."),
    };
    let amount = number
        .parse::<u64>()
        .map_err(|_| "Duration must be `10m`, `2h`, `1d`, `0`, or `clear`.")?;
    let seconds = amount
        .checked_mul(multiplier)
        .ok_or("Timeout duration cannot exceed 28 days.")?;
    if seconds == 0 {
        return Ok(None);
    }
    if seconds > MAX_TIMEOUT_SECONDS {
        return Err("Timeout duration cannot exceed 28 days.");
    }
    Ok(Some(Duration::from_secs(seconds)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_human_durations() {
        assert_eq!(
            parse_timeout_duration("10m"),
            Ok(Some(Duration::from_secs(600)))
        );
        assert_eq!(
            parse_timeout_duration("2h"),
            Ok(Some(Duration::from_secs(7_200)))
        );
        assert_eq!(
            parse_timeout_duration("1d"),
            Ok(Some(Duration::from_secs(86_400)))
        );
    }

    #[test]
    fn parses_timeout_clear_values() {
        assert_eq!(parse_timeout_duration("0"), Ok(None));
        assert_eq!(parse_timeout_duration("clear"), Ok(None));
    }

    #[test]
    fn rejects_garbage_and_more_than_twenty_eight_days() {
        assert!(parse_timeout_duration("forever").is_err());
        assert!(parse_timeout_duration("29d").is_err());
        assert_eq!(
            parse_timeout_duration("28d"),
            Ok(Some(Duration::from_secs(MAX_TIMEOUT_SECONDS)))
        );
    }
}
