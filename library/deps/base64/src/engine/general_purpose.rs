// `base64::engine::general_purpose`: the four preset engines. Encode is
// the standard 6-bit alphabet. Decode follows base64 0.22.1's padding and
// trailing-bit rules, so the error variant, offset, and byte match.

use crate::engine::Engine;
use crate::DecodeError;

const PAD: u8 = b'=';
const INVALID: u8 = 255;
const STD_ALPH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_ALPH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// A standard or URL-safe engine, with or without `=` padding.
#[derive(Clone)]
pub struct GeneralPurpose {
    url_safe: bool,
    pad: bool,
}

impl Engine for GeneralPurpose {}

/// Standard alphabet, `=` padding required.
pub const STANDARD: GeneralPurpose = GeneralPurpose { url_safe: false, pad: true };

/// Standard alphabet, no padding allowed.
pub const STANDARD_NO_PAD: GeneralPurpose = GeneralPurpose { url_safe: false, pad: false };

/// URL-safe alphabet (`-` and `_`), `=` padding required.
pub const URL_SAFE: GeneralPurpose = GeneralPurpose { url_safe: true, pad: true };

/// URL-safe alphabet, no padding allowed.
pub const URL_SAFE_NO_PAD: GeneralPurpose = GeneralPurpose { url_safe: true, pad: false };

impl GeneralPurpose {
    pub fn encode<T: AsRef<[u8]>>(&self, input: T) -> String {
        let input = input.as_ref();
        let alph = if self.url_safe { URL_ALPH } else { STD_ALPH };
        let mut out = String::new();
        let mut i = 0;
        while i + 3 <= input.len() {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
            out.push(alph[((n >> 18) & 63) as usize] as char);
            out.push(alph[((n >> 12) & 63) as usize] as char);
            out.push(alph[((n >> 6) & 63) as usize] as char);
            out.push(alph[(n & 63) as usize] as char);
            i += 3;
        }
        let rem = input.len() - i;
        if rem == 1 {
            let n = (input[i] as u32) << 16;
            out.push(alph[((n >> 18) & 63) as usize] as char);
            out.push(alph[((n >> 12) & 63) as usize] as char);
            if self.pad {
                out.push('=');
                out.push('=');
            }
        } else if rem == 2 {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
            out.push(alph[((n >> 18) & 63) as usize] as char);
            out.push(alph[((n >> 12) & 63) as usize] as char);
            out.push(alph[((n >> 6) & 63) as usize] as char);
            if self.pad {
                out.push('=');
            }
        }
        out
    }

    pub fn decode<T: AsRef<[u8]>>(&self, input: T) -> Result<Vec<u8>, DecodeError> {
        decode(input.as_ref(), self.url_safe, self.pad)
    }
}

fn decode_table(url_safe: bool) -> [u8; 256] {
    let alph = if url_safe { URL_ALPH } else { STD_ALPH };
    let mut table = [INVALID; 256];
    let mut i = 0;
    while i < 64 {
        table[alph[i] as usize] = i as u8;
        i += 1;
    }
    table
}

fn decode(input: &[u8], url_safe: bool, canonical_pad: bool) -> Result<Vec<u8>, DecodeError> {
    let table = decode_table(url_safe);
    let rem = input.len() % 4;
    if rem == 1 {
        let last = input[input.len() - 1];
        if last != PAD && table[last as usize] == INVALID {
            return Err(DecodeError::InvalidByte(input.len() - 1, last));
        }
    }
    let complete = input.len().saturating_sub(rem).saturating_sub(if rem == 0 { 4 } else { 0 });
    let extra = if rem > 0 { 1 } else { 0 };
    let est = (input.len() / 4 + extra) * 3;
    let mut output = vec![0u8; est];
    let mut out_i = 0;
    let mut in_i = 0;
    while in_i < complete {
        let c0 = morsel(&table, in_i, input[in_i])?;
        let c1 = morsel(&table, in_i + 1, input[in_i + 1])?;
        let c2 = morsel(&table, in_i + 2, input[in_i + 2])?;
        let c3 = morsel(&table, in_i + 3, input[in_i + 3])?;
        let n = (c0 << 18) | (c1 << 12) | (c2 << 6) | c3;
        output[out_i] = (n >> 16) as u8;
        output[out_i + 1] = (n >> 8) as u8;
        output[out_i + 2] = n as u8;
        in_i += 4;
        out_i += 3;
    }
    let input_index = complete;
    let mut morsels_in_leftover = 0;
    let mut padding_bytes = 0;
    let mut first_padding = 0;
    let mut last_symbol = 0u8;
    let mut morsels = [0u8; 4];
    let mut leftover = 0;
    while input_index + leftover < input.len() {
        let b = input[input_index + leftover];
        if b == PAD {
            if leftover < 2 {
                return Err(DecodeError::InvalidByte(input_index + leftover, b));
            }
            if padding_bytes == 0 {
                first_padding = leftover;
            }
            padding_bytes += 1;
            leftover += 1;
            continue;
        }
        if padding_bytes > 0 {
            return Err(DecodeError::InvalidByte(input_index + first_padding, PAD));
        }
        last_symbol = b;
        let value = table[b as usize];
        if value == INVALID {
            return Err(DecodeError::InvalidByte(input_index + leftover, b));
        }
        morsels[morsels_in_leftover] = value;
        morsels_in_leftover += 1;
        leftover += 1;
    }
    if !input.is_empty() && morsels_in_leftover < 2 {
        return Err(DecodeError::InvalidLength(input_index + morsels_in_leftover));
    }
    if canonical_pad {
        if (padding_bytes + morsels_in_leftover) % 4 != 0 {
            return Err(DecodeError::InvalidPadding);
        }
    } else if padding_bytes > 0 {
        return Err(DecodeError::InvalidPadding);
    }
    let leftover_bytes = morsels_in_leftover * 6 / 8;
    let mut leftover_num = (u32::from(morsels[0]) << 26) | (u32::from(morsels[1]) << 20) | (u32::from(morsels[2]) << 14) | (u32::from(morsels[3]) << 8);
    let mask = !0u32 >> ((leftover_bytes * 8) as u32);
    if (leftover_num & mask) != 0 {
        return Err(DecodeError::InvalidLastSymbol(input_index + morsels_in_leftover - 1, last_symbol));
    }
    let mut k = 0;
    while k < leftover_bytes {
        output[out_i] = (leftover_num >> 24) as u8;
        leftover_num <<= 8;
        out_i += 1;
        k += 1;
    }
    output.truncate(out_i);
    Ok(output)
}

fn morsel(table: &[u8; 256], offset: usize, byte: u8) -> Result<u32, DecodeError> {
    let value = table[byte as usize];
    if value == INVALID {
        return Err(DecodeError::InvalidByte(offset, byte));
    }
    Ok(u32::from(value))
}
