use std::io::{self, Write};

use anyhow::{Context, Result};
use log::info;

use super::state::UpdateDecision;

pub fn prompt_for_update() -> Result<UpdateDecision> {
    loop {
        print!("Would you like to update now? [yes/postpone/discard]: ");
        io::stdout().flush().context("failed to flush stdout")?;
        let mut input = String::new();
        let bytes = io::stdin()
            .read_line(&mut input)
            .context("failed to read user input")?;

        if bytes == 0 {
            info!("No input received; treating as discard.");
            return Ok(UpdateDecision::Discard);
        }

        match input.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(UpdateDecision::UpdateNow),
            "p" | "postpone" => return Ok(UpdateDecision::Postpone),
            "d" | "discard" | "n" | "no" => return Ok(UpdateDecision::Discard),
            _ => {
                println!("Please enter 'yes', 'postpone', or 'discard'.");
            }
        }
    }
}
