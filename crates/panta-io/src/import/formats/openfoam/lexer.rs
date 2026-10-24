//! OpenFOAM 字典文件（FoamFile 头 + 列表 / 字典）的字节级词法与列表读取。
//!
//! 只覆盖工具写出的 polyMesh 与场文件语法：注释、`N(…)` / `N{v}` 列表、ASCII 与
//! binary 列表体（`arch` 声明 label / scalar 位宽）。宏、`#include`、`$var` 不展开，
//! 遇到时由调用方按「无法识别」整体拒绝。

use super::super::{FormatError, FormatResult};

/// FoamFile 头中与解析有关的字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Header {
    pub binary: bool,
    pub class: String,
    pub label_bytes: usize,
    pub scalar_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Token<'a> {
    Punct(u8),
    Word(&'a str),
    Text(&'a str),
}

pub(crate) struct Lexer<'a> {
    bytes: &'a [u8],
    position: usize,
    pub header: Header,
}

fn invalid(message: impl Into<String>) -> FormatError {
    FormatError::new(message)
}

const PUNCT: &[u8] = b"(){}[];";

impl<'a> Lexer<'a> {
    /// 读取 FoamFile 头；缺头时按 ASCII、32 位 label、64 位 scalar（OpenFOAM 默认）。
    pub fn new(bytes: &'a [u8]) -> FormatResult<Self> {
        let mut lexer = Self {
            bytes,
            position: 0,
            header: Header {
                binary: false,
                class: String::new(),
                label_bytes: 4,
                scalar_bytes: 8,
            },
        };
        let start = lexer.position;
        if lexer.next()? == Some(Token::Word("FoamFile")) {
            lexer.expect(b'{')?;
            loop {
                match lexer.next()? {
                    Some(Token::Punct(b'}')) => break,
                    Some(Token::Word(key)) => {
                        let value = lexer.entry_text()?;
                        lexer.apply_header(key, value)?;
                    }
                    _ => return Err(invalid("malformed FoamFile header")),
                }
            }
        } else {
            lexer.position = start;
        }
        Ok(lexer)
    }

    fn apply_header(&mut self, key: &str, value: &str) -> FormatResult<()> {
        match key {
            "format" => match value {
                "ascii" => self.header.binary = false,
                "binary" => self.header.binary = true,
                other => return Err(invalid(format!("unsupported FoamFile format {other}"))),
            },
            "class" => self.header.class = value.to_owned(),
            "arch" => {
                for part in value.split(';') {
                    let bits = |prefix: &str| {
                        part.trim()
                            .strip_prefix(prefix)
                            .and_then(|bits| bits.parse::<usize>().ok())
                    };
                    if let Some(bits) = bits("label=") {
                        self.header.label_bytes = bits / 8;
                    } else if let Some(bits) = bits("scalar=") {
                        self.header.scalar_bytes = bits / 8;
                    } else if part.trim() == "MSB" {
                        return Err(invalid(
                            "big-endian OpenFOAM binary files are not supported",
                        ));
                    }
                }
                if !matches!(self.header.label_bytes, 4 | 8)
                    || !matches!(self.header.scalar_bytes, 4 | 8)
                {
                    return Err(invalid(format!("unsupported arch {value}")));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 读取一个头部条目值（到 `;`），字符串去掉引号；多词值以首个词为准。
    fn entry_text(&mut self) -> FormatResult<&'a str> {
        let mut value = "";
        loop {
            match self.next()? {
                Some(Token::Punct(b';')) => return Ok(value),
                Some(Token::Word(word)) | Some(Token::Text(word)) => {
                    if value.is_empty() {
                        value = word;
                    }
                }
                _ => return Err(invalid("malformed FoamFile header entry")),
            }
        }
    }

    fn skip_space_and_comments(&mut self) {
        loop {
            while self
                .bytes
                .get(self.position)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.position += 1;
            }
            let rest = &self.bytes[self.position.min(self.bytes.len())..];
            if rest.starts_with(b"//") {
                let end = rest.iter().position(|&byte| byte == b'\n');
                self.position += end.map_or(rest.len(), |end| end + 1);
            } else if rest.starts_with(b"/*") {
                let end = rest
                    .windows(2)
                    .skip(2)
                    .position(|pair| pair == b"*/")
                    .map_or(rest.len(), |end| end + 4);
                self.position += end;
            } else {
                return;
            }
        }
    }

    pub fn next(&mut self) -> FormatResult<Option<Token<'a>>> {
        self.skip_space_and_comments();
        let Some(&byte) = self.bytes.get(self.position) else {
            return Ok(None);
        };
        if PUNCT.contains(&byte) {
            self.position += 1;
            return Ok(Some(Token::Punct(byte)));
        }
        if byte == b'"' {
            let start = self.position + 1;
            let end = self.bytes[start..]
                .iter()
                .position(|&byte| byte == b'"')
                .ok_or_else(|| invalid("unterminated string"))?;
            self.position = start + end + 1;
            return Ok(Some(Token::Text(utf8(&self.bytes[start..start + end])?)));
        }
        let start = self.position;
        while self.bytes.get(self.position).is_some_and(|&byte| {
            !byte.is_ascii_whitespace() && !PUNCT.contains(&byte) && byte != b'"'
        }) {
            self.position += 1;
        }
        Ok(Some(Token::Word(utf8(&self.bytes[start..self.position])?)))
    }

    pub fn expect(&mut self, punct: u8) -> FormatResult<()> {
        match self.next()? {
            Some(Token::Punct(found)) if found == punct => Ok(()),
            other => Err(invalid(format!(
                "expected '{}', found {other:?}",
                punct as char
            ))),
        }
    }

    pub fn word(&mut self) -> FormatResult<&'a str> {
        match self.next()? {
            Some(Token::Word(word)) => Ok(word),
            other => Err(invalid(format!("expected a word, found {other:?}"))),
        }
    }

    pub fn number(&mut self) -> FormatResult<f64> {
        let word = self.word()?;
        word.parse::<f64>()
            .map_err(|_| invalid(format!("expected a number, found {word}")))
    }

    pub fn label(&mut self) -> FormatResult<usize> {
        let word = self.word()?;
        word.parse::<usize>()
            .map_err(|_| invalid(format!("expected a label, found {word}")))
    }

    /// 跳过当前条目剩余部分（含嵌套括号）直到顶层 `;` 或子字典结束。
    pub fn skip_entry(&mut self) -> FormatResult<()> {
        let mut depth = 0usize;
        loop {
            match self.next()? {
                None => return Err(invalid("unexpected end of file")),
                Some(Token::Punct(b'(' | b'{' | b'[')) => depth += 1,
                Some(Token::Punct(closing @ (b')' | b'}' | b']'))) => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| invalid("unbalanced brackets"))?;
                    // 子字典条目 `key { … }` 没有结尾分号。
                    if depth == 0 && closing == b'}' {
                        return Ok(());
                    }
                }
                Some(Token::Punct(b';')) if depth == 0 => return Ok(()),
                Some(_) => {}
            }
        }
    }

    pub fn peek_punct(&mut self) -> Option<u8> {
        self.skip_space_and_comments();
        self.bytes
            .get(self.position)
            .copied()
            .filter(|byte| PUNCT.contains(byte))
    }

    /// 读取列表前缀：返回元素个数与形态。允许 `N(`、`N{`、无个数的 ASCII `(`。
    fn list_start(&mut self) -> FormatResult<ListStart> {
        match self.next()? {
            Some(Token::Word(word)) => {
                let count = word
                    .parse::<usize>()
                    .map_err(|_| invalid(format!("expected a list size, found {word}")))?;
                match self.peek_punct() {
                    Some(b'(') => {
                        self.position += 1;
                        Ok(ListStart::Explicit(count))
                    }
                    Some(b'{') => {
                        self.position += 1;
                        Ok(ListStart::Uniform(count))
                    }
                    _ => Err(invalid("expected '(' or '{' after list size")),
                }
            }
            Some(Token::Punct(b'(')) if !self.header.binary => Ok(ListStart::Unsized),
            other => Err(invalid(format!("expected a list, found {other:?}"))),
        }
    }

    fn raw(&mut self, length: usize) -> FormatResult<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| invalid("binary list is truncated"))?;
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn decode_label(&self, bytes: &[u8]) -> FormatResult<u64> {
        let value = if self.header.label_bytes == 4 {
            i64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            let mut word = [0u8; 8];
            word.copy_from_slice(&bytes[..8]);
            i64::from_le_bytes(word)
        };
        u64::try_from(value).map_err(|_| invalid("negative label"))
    }

    fn decode_scalar(&self, bytes: &[u8]) -> f64 {
        if self.header.scalar_bytes == 4 {
            f64::from(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            let mut word = [0u8; 8];
            word.copy_from_slice(&bytes[..8]);
            f64::from_le_bytes(word)
        }
    }

    /// 读取 label 列表（labelList）。
    pub fn labels(&mut self) -> FormatResult<Vec<u64>> {
        match self.list_start()? {
            ListStart::Explicit(count) if self.header.binary => {
                let width = self.header.label_bytes;
                let bytes = self.raw(
                    count
                        .checked_mul(width)
                        .ok_or_else(|| invalid("list size overflow"))?,
                )?;
                let values = bytes
                    .chunks_exact(width)
                    .map(|chunk| self.decode_label(chunk))
                    .collect::<FormatResult<Vec<_>>>()?;
                self.expect(b')')?;
                Ok(values)
            }
            ListStart::Explicit(count) => {
                let mut values = Vec::with_capacity(count.min(1 << 24));
                for _ in 0..count {
                    values.push(self.label()? as u64);
                }
                self.expect(b')')?;
                Ok(values)
            }
            ListStart::Uniform(count) => {
                let value = if self.header.binary {
                    let bytes = self.raw(self.header.label_bytes)?;
                    self.decode_label(bytes)?
                } else {
                    self.label()? as u64
                };
                self.expect(b'}')?;
                Ok(vec![value; count])
            }
            ListStart::Unsized => {
                let mut values = Vec::new();
                loop {
                    match self.next()? {
                        Some(Token::Punct(b')')) => return Ok(values),
                        Some(Token::Word(word)) => values.push(
                            word.parse::<u64>()
                                .map_err(|_| invalid(format!("expected a label, found {word}")))?,
                        ),
                        other => return Err(invalid(format!("unexpected {other:?} in list"))),
                    }
                }
            }
        }
    }

    /// 读取每元素 `components` 个标量的列表（scalarList：1；vectorList：3，ASCII 元素为 `(x y z)`）。
    pub fn scalars(&mut self, components: usize) -> FormatResult<Vec<f64>> {
        match self.list_start()? {
            ListStart::Explicit(count) if self.header.binary => {
                let width = self.header.scalar_bytes;
                let length = count
                    .checked_mul(components)
                    .and_then(|values| values.checked_mul(width))
                    .ok_or_else(|| invalid("list size overflow"))?;
                let bytes = self.raw(length)?;
                let values = bytes
                    .chunks_exact(width)
                    .map(|chunk| self.decode_scalar(chunk))
                    .collect();
                self.expect(b')')?;
                Ok(values)
            }
            ListStart::Explicit(count) => {
                let mut values = Vec::with_capacity(count.saturating_mul(components).min(1 << 24));
                for _ in 0..count {
                    self.ascii_tuple(components, &mut values)?;
                }
                self.expect(b')')?;
                Ok(values)
            }
            ListStart::Uniform(count) => {
                let mut tuple = Vec::with_capacity(components);
                if self.header.binary {
                    let bytes = self.raw(components * self.header.scalar_bytes)?;
                    tuple.extend(
                        bytes
                            .chunks_exact(self.header.scalar_bytes)
                            .map(|chunk| self.decode_scalar(chunk)),
                    );
                } else {
                    self.ascii_tuple(components, &mut tuple)?;
                }
                self.expect(b'}')?;
                Ok(tuple.repeat(count))
            }
            ListStart::Unsized => {
                let mut values = Vec::new();
                while self.peek_punct() != Some(b')') {
                    self.ascii_tuple(components, &mut values)?;
                }
                self.position += 1;
                Ok(values)
            }
        }
    }

    /// ASCII 单个元素：标量为一个数，多分量为 `(a b c)`。
    pub fn ascii_tuple(&mut self, components: usize, out: &mut Vec<f64>) -> FormatResult<()> {
        if components == 1 {
            out.push(self.number()?);
            return Ok(());
        }
        self.expect(b'(')?;
        for _ in 0..components {
            out.push(self.number()?);
        }
        self.expect(b')')
    }

    /// 面列表：faceList（ASCII `n(a b …)`；旧式 binary 每面 `n(` 原始字节 `)`）或
    /// faceCompactList（偏移 labelList + 节点 labelList）。返回 (节点, 结束偏移)。
    pub fn faces(&mut self) -> FormatResult<(Vec<u64>, Vec<usize>)> {
        if self.header.class == "faceCompactList" {
            let offsets = self.labels()?;
            let nodes = self.labels()?;
            let mut ends = Vec::with_capacity(offsets.len().saturating_sub(1));
            let mut previous = 0usize;
            for (index, &offset) in offsets.iter().enumerate() {
                let offset = usize::try_from(offset)
                    .ok()
                    .filter(|&offset| offset >= previous && offset <= nodes.len())
                    .ok_or_else(|| invalid("face offsets out of range"))?;
                if index == 0 {
                    if offset != 0 {
                        return Err(invalid("face offsets must start at 0"));
                    }
                } else {
                    ends.push(offset);
                }
                previous = offset;
            }
            if previous != nodes.len() {
                return Err(invalid("face offsets do not cover the node list"));
            }
            return Ok((nodes, ends));
        }
        let count = match self.list_start()? {
            ListStart::Explicit(count) => count,
            _ => return Err(invalid("face list must declare its size")),
        };
        let mut nodes = Vec::new();
        let mut ends = Vec::with_capacity(count.min(1 << 24));
        for _ in 0..count {
            nodes.extend(self.labels()?);
            ends.push(nodes.len());
        }
        self.expect(b')')?;
        Ok((nodes, ends))
    }
}

enum ListStart {
    Explicit(usize),
    Uniform(usize),
    Unsized,
}

fn utf8(bytes: &[u8]) -> FormatResult<&str> {
    std::str::from_utf8(bytes).map_err(|_| invalid("token is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_lists_comments_and_uniform_forms() -> FormatResult<()> {
        let text = b"/* banner */ FoamFile { format ascii; class labelList; \
            arch \"LSB;label=32;scalar=64\"; } // c\n 3(1 2 3) 2{7} (4 5) \
            2((0 1 2) (3 4 5)) 3{1.5}";
        let mut lexer = Lexer::new(text)?;
        assert_eq!(lexer.header.class, "labelList");
        assert_eq!(lexer.labels()?, vec![1, 2, 3]);
        assert_eq!(lexer.labels()?, vec![7, 7]);
        assert_eq!(lexer.labels()?, vec![4, 5]);
        assert_eq!(lexer.scalars(3)?, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(lexer.scalars(1)?, vec![1.5; 3]);
        assert_eq!(lexer.next()?, None);
        Ok(())
    }

    #[test]
    fn binary_lists_follow_declared_widths() -> FormatResult<()> {
        let mut bytes =
            b"FoamFile { format binary; class labelList; arch \"LSB;label=64;scalar=32\"; }\n2("
                .to_vec();
        bytes.extend(5i64.to_le_bytes());
        bytes.extend(9i64.to_le_bytes());
        bytes.extend(b")\n1(");
        bytes.extend(2.5f32.to_le_bytes());
        bytes.extend(b")");
        let mut lexer = Lexer::new(&bytes)?;
        assert_eq!(lexer.labels()?, vec![5, 9]);
        assert_eq!(lexer.scalars(1)?, vec![2.5]);
        let mut truncated = b"FoamFile { format binary; class labelList; }\n3(".to_vec();
        truncated.extend(1i32.to_le_bytes());
        assert!(Lexer::new(&truncated)?.labels().is_err());
        assert!(Lexer::new(b"FoamFile { format binary; arch \"MSB;label=32\"; }").is_err());
        Ok(())
    }
}
