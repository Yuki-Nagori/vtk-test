//! 文本 / 二进制混合格式（旧版 VTK、PLY）共用的字节游标。

use super::{FormatError, FormatResult};

pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub fn is_at_end(&mut self) -> bool {
        self.skip_whitespace();
        self.position >= self.bytes.len()
    }

    fn skip_whitespace(&mut self) {
        while self
            .bytes
            .get(self.position)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            self.position += 1;
        }
    }

    /// 读取一行（不含行尾），用于头部和注释。
    pub fn line(&mut self) -> FormatResult<&'a str> {
        if self.position >= self.bytes.len() {
            return Err(FormatError::new("unexpected end of file"));
        }
        let rest = &self.bytes[self.position..];
        let end = rest
            .iter()
            .position(|&byte| byte == b'\n')
            .unwrap_or(rest.len());
        self.position += (end + 1).min(rest.len());
        let line = rest[..end].strip_suffix(b"\r").unwrap_or(&rest[..end]);
        std::str::from_utf8(line).map_err(|_| FormatError::new("header is not UTF-8"))
    }

    pub fn token(&mut self) -> FormatResult<&'a str> {
        self.skip_whitespace();
        let start = self.position;
        while self
            .bytes
            .get(self.position)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            self.position += 1;
        }
        if start == self.position {
            return Err(FormatError::new("unexpected end of file"));
        }
        std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| FormatError::new("token is not UTF-8"))
    }

    pub fn parse<T: std::str::FromStr>(&mut self) -> FormatResult<T> {
        let token = self.token()?;
        token
            .parse()
            .map_err(|_| FormatError::new(format!("invalid number {token:?}")))
    }

    /// 跳过关键字行剩余部分直到换行；二进制块紧随该换行开始。
    pub fn finish_line(&mut self) -> FormatResult<()> {
        self.line().map(|_| ())
    }

    pub fn raw(&mut self, length: usize) -> FormatResult<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| FormatError::new("binary block exceeds file size"))?;
        let block = &self.bytes[self.position..end];
        self.position = end;
        Ok(block)
    }
}

/// 二进制标量类型及字节序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
}

impl Scalar {
    pub fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
        }
    }

    /// 把一个标量解码为 f64；整数索引另走 [`Scalar::decode_index`] 保留全部精度。
    pub fn decode_f64(self, bytes: &[u8], little_endian: bool) -> f64 {
        macro_rules! read {
            ($ty:ty) => {{
                let mut array = [0u8; std::mem::size_of::<$ty>()];
                array.copy_from_slice(&bytes[..std::mem::size_of::<$ty>()]);
                if little_endian {
                    <$ty>::from_le_bytes(array) as f64
                } else {
                    <$ty>::from_be_bytes(array) as f64
                }
            }};
        }
        match self {
            Self::I8 => read!(i8),
            Self::U8 => read!(u8),
            Self::I16 => read!(i16),
            Self::U16 => read!(u16),
            Self::I32 => read!(i32),
            Self::U32 => read!(u32),
            Self::I64 => read!(i64),
            Self::U64 => read!(u64),
            Self::F32 => read!(f32),
            Self::F64 => read!(f64),
        }
    }

    pub fn decode_index(self, bytes: &[u8], little_endian: bool) -> FormatResult<u64> {
        macro_rules! read {
            ($ty:ty) => {{
                let mut array = [0u8; std::mem::size_of::<$ty>()];
                array.copy_from_slice(&bytes[..std::mem::size_of::<$ty>()]);
                let value = if little_endian {
                    <$ty>::from_le_bytes(array)
                } else {
                    <$ty>::from_be_bytes(array)
                };
                u64::try_from(value).map_err(|_| FormatError::new("negative index"))
            }};
        }
        match self {
            Self::I8 => read!(i8),
            Self::U8 => read!(u8),
            Self::I16 => read!(i16),
            Self::U16 => read!(u16),
            Self::I32 => read!(i32),
            Self::U32 => read!(u32),
            Self::I64 => read!(i64),
            Self::U64 => read!(u64),
            Self::F32 | Self::F64 => Err(FormatError::new("floating-point index array")),
        }
    }
}

/// 解码连续二进制数组为 f64，长度不足时报错。
pub(crate) fn decode_f64_array(
    bytes: &[u8],
    scalar: Scalar,
    count: usize,
    little_endian: bool,
) -> FormatResult<Vec<f64>> {
    let needed = count
        .checked_mul(scalar.size())
        .filter(|&needed| needed <= bytes.len())
        .ok_or_else(|| FormatError::new("binary array is truncated"))?;
    Ok(bytes[..needed]
        .chunks_exact(scalar.size())
        .map(|chunk| scalar.decode_f64(chunk, little_endian))
        .collect())
}

pub(crate) fn decode_index_array(
    bytes: &[u8],
    scalar: Scalar,
    count: usize,
    little_endian: bool,
) -> FormatResult<Vec<u64>> {
    let needed = count
        .checked_mul(scalar.size())
        .filter(|&needed| needed <= bytes.len())
        .ok_or_else(|| FormatError::new("binary array is truncated"))?;
    bytes[..needed]
        .chunks_exact(scalar.size())
        .map(|chunk| scalar.decode_index(chunk, little_endian))
        .collect()
}

/// 把三元组坐标数组转为点表并拒绝非有限值。
pub(crate) fn points_from_flat(values: &[f64]) -> FormatResult<Vec<[f64; 3]>> {
    if !values.len().is_multiple_of(3) {
        return Err(FormatError::new(
            "point array length is not a multiple of 3",
        ));
    }
    values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|chunk| super::checked_point(*chunk))
        .collect()
}
