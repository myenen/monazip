use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Model {
    Raw,
    Delta,
    StrideDelta,
    StrideXor,
    Linear2,
    Spatial,
    Transpose,
    BitShuffle,
    Constant,
    Periodic,
}

impl Model {
    pub(crate) fn from_id(id: u8) -> io::Result<Self> {
        use Model::*;
        [
            Raw,
            Delta,
            StrideDelta,
            StrideXor,
            Linear2,
            Spatial,
            Transpose,
            BitShuffle,
            Constant,
            Periodic,
        ]
        .get(id as usize)
        .copied()
        .ok_or_else(|| invalid("Bilinmeyen model"))
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Raw => "RAW",
            Self::Delta => "DELTA",
            Self::StrideDelta => "STRIDE_DELTA",
            Self::StrideXor => "STRIDE_XOR",
            Self::Linear2 => "LINEAR_2",
            Self::Spatial => "SPATIAL",
            Self::Transpose => "TRANSPOSE",
            Self::BitShuffle => "BIT_SHUFFLE",
            Self::Constant => "CONSTANT",
            Self::Periodic => "PERIODIC",
        }
    }

    pub(crate) fn uses_width(self) -> bool {
        matches!(
            self,
            Self::StrideDelta | Self::StrideXor | Self::Spatial | Self::Transpose
        )
    }

    pub(crate) fn is_pattern(self) -> bool {
        matches!(self, Self::Constant | Self::Periodic)
    }

    pub(crate) fn validate(self, params: &[u8], len: usize) -> io::Result<usize> {
        if self.uses_width() {
            let encoded = params
                .try_into()
                .map_err(|_| invalid("Geçersiz komşuluk parametresi"))?;
            let width = u16::from_le_bytes(encoded) as usize;
            if width == 0
                || width >= len
                || (width == 1 && matches!(self, Self::Spatial | Self::Transpose))
            {
                return Err(invalid("Komşuluk mesafesi blok dışında"));
            }
            return Ok(width);
        }
        let valid = match self {
            Self::Constant => params.len() == 1,
            Self::Periodic => (2..=1024).contains(&params.len()) && params.len() < len,
            _ => params.is_empty(),
        };
        if !valid {
            return Err(invalid("Geçersiz model parametreleri"));
        }
        Ok(0)
    }
}

pub(crate) fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(crate) fn period(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let mut prefix = vec![0; bytes.len()];
    for i in 1..bytes.len() {
        let mut matched = prefix[i - 1];
        while matched != 0 && bytes[i] != bytes[matched] {
            matched = prefix[matched - 1];
        }
        prefix[i] = matched + usize::from(bytes[i] == bytes[matched]);
    }
    bytes.len() - prefix[bytes.len() - 1]
}
