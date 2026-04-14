use std::fmt::{self, Display};
use std::str::FromStr;

/// GTKWave/FST attribute type.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum AttributeType {
    /// Miscellaneous attributes (subtypes identified by hex code).
    Misc,
    /// Array attributes.
    Array,
    /// Enum attributes.
    Enum,
    /// Pack/class attributes.
    Pack,
}

super::unit_error_struct!(InvalidAttributeType, "invalid attribute type");

impl FromStr for AttributeType {
    type Err = InvalidAttributeType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "misc" => Ok(AttributeType::Misc),
            "array" => Ok(AttributeType::Array),
            "enum" => Ok(AttributeType::Enum),
            "class" | "pack" => Ok(AttributeType::Pack),
            _ => Err(InvalidAttributeType),
        }
    }
}

impl Display for AttributeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                AttributeType::Misc => "misc",
                AttributeType::Array => "array",
                AttributeType::Enum => "enum",
                AttributeType::Pack => "class",
            }
        )
    }
}

/// Misc attribute subtype (two-digit hex in VCD text).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum MiscAttributeSubtype {
    /// Comment (hex 00). Normally rendered as `$comment`, not `$attrbegin`.
    Comment,
    /// Environment variable (hex 01).
    EnvVar,
    /// Supplemental variable type info, e.g. VHDL type (hex 02).
    SupVar,
    /// Path name (hex 03).
    PathName,
    /// Source stem (hex 04).
    SourceStem,
    /// Source instantiation stem (hex 05).
    SourceInstantiationStem,
    /// Value list (hex 06).
    ValueList,
    /// Enum table definition or reference (hex 07).
    EnumTable,
    /// Unknown misc attribute (hex 08).
    Unknown,
}

super::unit_error_struct!(InvalidMiscAttributeSubtype, "invalid misc attribute subtype");

impl MiscAttributeSubtype {
    /// Parse from the two-digit hex string used in VCD text.
    pub fn from_hex(s: &str) -> Result<Self, InvalidMiscAttributeSubtype> {
        match s {
            "00" => Ok(MiscAttributeSubtype::Comment),
            "01" => Ok(MiscAttributeSubtype::EnvVar),
            "02" => Ok(MiscAttributeSubtype::SupVar),
            "03" => Ok(MiscAttributeSubtype::PathName),
            "04" => Ok(MiscAttributeSubtype::SourceStem),
            "05" => Ok(MiscAttributeSubtype::SourceInstantiationStem),
            "06" => Ok(MiscAttributeSubtype::ValueList),
            "07" => Ok(MiscAttributeSubtype::EnumTable),
            "08" => Ok(MiscAttributeSubtype::Unknown),
            _ => Err(InvalidMiscAttributeSubtype),
        }
    }

    /// Returns the two-digit hex string for this subtype.
    pub fn to_hex(self) -> &'static str {
        match self {
            MiscAttributeSubtype::Comment => "00",
            MiscAttributeSubtype::EnvVar => "01",
            MiscAttributeSubtype::SupVar => "02",
            MiscAttributeSubtype::PathName => "03",
            MiscAttributeSubtype::SourceStem => "04",
            MiscAttributeSubtype::SourceInstantiationStem => "05",
            MiscAttributeSubtype::ValueList => "06",
            MiscAttributeSubtype::EnumTable => "07",
            MiscAttributeSubtype::Unknown => "08",
        }
    }
}

impl Display for MiscAttributeSubtype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Array attribute subtype.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum ArrayType {
    /// No array type.
    None,
    /// Unpacked array.
    Unpacked,
    /// Packed array.
    Packed,
    /// Sparse array.
    Sparse,
}

super::unit_error_struct!(InvalidArrayType, "invalid array type");

impl FromStr for ArrayType {
    type Err = InvalidArrayType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(ArrayType::None),
            "unpacked" => Ok(ArrayType::Unpacked),
            "packed" => Ok(ArrayType::Packed),
            "sparse" => Ok(ArrayType::Sparse),
            _ => Err(InvalidArrayType),
        }
    }
}

impl Display for ArrayType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                ArrayType::None => "none",
                ArrayType::Unpacked => "unpacked",
                ArrayType::Packed => "packed",
                ArrayType::Sparse => "sparse",
            }
        )
    }
}

/// Enum value type used in enum attribute subtypes.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum EnumValueType {
    /// `integer`
    Integer,
    /// `bit`
    Bit,
    /// `logic`
    Logic,
    /// `int`
    Int,
    /// `shortint`
    ShortInt,
    /// `longint`
    LongInt,
    /// `byte`
    Byte,
    /// `unsigned_integer`
    UnsignedInteger,
    /// `unsigned_bit`
    UnsignedBit,
    /// `unsigned_logic`
    UnsignedLogic,
    /// `unsigned_int`
    UnsignedInt,
    /// `unsigned_shortint`
    UnsignedShortInt,
    /// `unsigned_longint`
    UnsignedLongInt,
    /// `unsigned_byte`
    UnsignedByte,
}

super::unit_error_struct!(InvalidEnumValueType, "invalid enum value type");

impl FromStr for EnumValueType {
    type Err = InvalidEnumValueType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "integer" => Ok(EnumValueType::Integer),
            "bit" => Ok(EnumValueType::Bit),
            "logic" => Ok(EnumValueType::Logic),
            "int" => Ok(EnumValueType::Int),
            "shortint" => Ok(EnumValueType::ShortInt),
            "longint" => Ok(EnumValueType::LongInt),
            "byte" => Ok(EnumValueType::Byte),
            "unsigned_integer" => Ok(EnumValueType::UnsignedInteger),
            "unsigned_bit" => Ok(EnumValueType::UnsignedBit),
            "unsigned_logic" => Ok(EnumValueType::UnsignedLogic),
            "unsigned_int" => Ok(EnumValueType::UnsignedInt),
            "unsigned_shortint" => Ok(EnumValueType::UnsignedShortInt),
            "unsigned_longint" => Ok(EnumValueType::UnsignedLongInt),
            "unsigned_byte" => Ok(EnumValueType::UnsignedByte),
            _ => Err(InvalidEnumValueType),
        }
    }
}

impl Display for EnumValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                EnumValueType::Integer => "integer",
                EnumValueType::Bit => "bit",
                EnumValueType::Logic => "logic",
                EnumValueType::Int => "int",
                EnumValueType::ShortInt => "shortint",
                EnumValueType::LongInt => "longint",
                EnumValueType::Byte => "byte",
                EnumValueType::UnsignedInteger => "unsigned_integer",
                EnumValueType::UnsignedBit => "unsigned_bit",
                EnumValueType::UnsignedLogic => "unsigned_logic",
                EnumValueType::UnsignedInt => "unsigned_int",
                EnumValueType::UnsignedShortInt => "unsigned_shortint",
                EnumValueType::UnsignedLongInt => "unsigned_longint",
                EnumValueType::UnsignedByte => "unsigned_byte",
            }
        )
    }
}

/// Pack/class attribute subtype.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum PackType {
    /// No packing.
    None,
    /// Unpacked.
    Unpacked,
    /// Packed.
    Packed,
    /// Tagged packed.
    TaggedPacked,
}

super::unit_error_struct!(InvalidPackType, "invalid pack type");

impl FromStr for PackType {
    type Err = InvalidPackType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(PackType::None),
            "unpacked" => Ok(PackType::Unpacked),
            "packed" => Ok(PackType::Packed),
            "tagged_packed" => Ok(PackType::TaggedPacked),
            _ => Err(InvalidPackType),
        }
    }
}

impl Display for PackType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                PackType::None => "none",
                PackType::Unpacked => "unpacked",
                PackType::Packed => "packed",
                PackType::TaggedPacked => "tagged_packed",
            }
        )
    }
}

/// A parsed GTKWave/FST attribute from `$attrbegin ... $end`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Attribute {
    /// The attribute type.
    pub attr_type: AttributeType,
    /// The subtype string as it appeared in the VCD text.
    pub subtype: String,
    /// The name field.
    pub name: String,
    /// The numeric argument.
    pub arg: i64,
}

impl Attribute {
    /// Create a new `Attribute`.
    pub fn new(attr_type: AttributeType, subtype: String, name: String, arg: i64) -> Self {
        Self { attr_type, subtype, name, arg }
    }
}
