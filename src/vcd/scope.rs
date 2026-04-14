use std::fmt::{self, Display};
use std::str::FromStr;

use super::IdCode;
use super::Attribute;

/// A type of scope, as used in the `$scope` command.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum ScopeType {
    Module,
    Task,
    Function,
    Begin,
    Fork,
    Generate,
    Struct,
    Union,
    Class,
    Interface,
    Package,
    Program,
    VhdlArchitecture,
    VhdlProcedure,
    VhdlFunction,
    VhdlRecord,
    VhdlProcess,
    VhdlBlock,
    VhdlForGenerate,
    VhdlIfGenerate,
    VhdlGenerate,
    VhdlPackage,
    SvArray,
}

super::unit_error_struct!(InvalidScopeType, "invalid scope type");

impl ScopeType {
    /// Parse a scope type string, optionally accepting GTKWave extension types.
    pub fn from_str_ext(s: &str, gtkwave_extensions: bool) -> Result<Self, InvalidScopeType> {
        use ScopeType::*;
        match s {
            "module" => Ok(Module),
            "task" => Ok(Task),
            "function" => Ok(Function),
            "begin" => Ok(Begin),
            "fork" => Ok(Fork),
            _ if gtkwave_extensions => match s {
                "generate" => Ok(Generate),
                "struct" => Ok(Struct),
                "union" => Ok(Union),
                "class" => Ok(Class),
                "interface" => Ok(Interface),
                "package" => Ok(Package),
                "program" => Ok(Program),
                "vhdl_architecture" => Ok(VhdlArchitecture),
                "vhdl_procedure" => Ok(VhdlProcedure),
                "vhdl_function" => Ok(VhdlFunction),
                "vhdl_record" => Ok(VhdlRecord),
                "vhdl_process" => Ok(VhdlProcess),
                "vhdl_block" => Ok(VhdlBlock),
                "vhdl_for_generate" => Ok(VhdlForGenerate),
                "vhdl_if_generate" => Ok(VhdlIfGenerate),
                "vhdl_generate" => Ok(VhdlGenerate),
                "vhdl_package" => Ok(VhdlPackage),
                "sv_array" => Ok(SvArray),
                _ => Err(InvalidScopeType),
            },
            _ => Err(InvalidScopeType),
        }
    }
}

impl FromStr for ScopeType {
    type Err = InvalidScopeType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ScopeType::from_str_ext(s, true)
    }
}

impl Display for ScopeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ScopeType::*;
        write!(
            f,
            "{}",
            match *self {
                Module => "module",
                Task => "task",
                Function => "function",
                Begin => "begin",
                Fork => "fork",
                Generate => "generate",
                Struct => "struct",
                Union => "union",
                Class => "class",
                Interface => "interface",
                Package => "package",
                Program => "program",
                VhdlArchitecture => "vhdl_architecture",
                VhdlProcedure => "vhdl_procedure",
                VhdlFunction => "vhdl_function",
                VhdlRecord => "vhdl_record",
                VhdlProcess => "vhdl_process",
                VhdlBlock => "vhdl_block",
                VhdlForGenerate => "vhdl_for_generate",
                VhdlIfGenerate => "vhdl_if_generate",
                VhdlGenerate => "vhdl_generate",
                VhdlPackage => "vhdl_package",
                SvArray => "sv_array",
            }
        )
    }
}

/// A type of variable, as used in the `$var` command.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum VarType {
    Event,
    Integer,
    Parameter,
    Real,
    Reg,
    Supply0,
    Supply1,
    Time,
    Tri,
    TriAnd,
    TriOr,
    TriReg,
    Tri0,
    Tri1,
    WAnd,
    Wire,
    WOr,
    String,
    Port,
    SparseArray,
    RealTime,
    Bit,
    Logic,
    Int,
    ShortInt,
    LongInt,
    Byte,
    Enum,
    ShortReal,
    RealParameter,
}

super::unit_error_struct!(InvalidVarType, "invalid variable type");

impl VarType {
    /// Parse a var type string, optionally accepting GTKWave extension types.
    pub fn from_str_ext(s: &str, gtkwave_extensions: bool) -> Result<Self, InvalidVarType> {
        use VarType::*;
        match s {
            "event" => Ok(Event),
            "integer" => Ok(Integer),
            "parameter" => Ok(Parameter),
            "real" => Ok(Real),
            "reg" => Ok(Reg),
            "supply0" => Ok(Supply0),
            "supply1" => Ok(Supply1),
            "time" => Ok(Time),
            "tri" => Ok(Tri),
            "triand" => Ok(TriAnd),
            "trior" => Ok(TriOr),
            "trireg" => Ok(TriReg),
            "tri0" => Ok(Tri0),
            "tri1" => Ok(Tri1),
            "wand" => Ok(WAnd),
            "wire" => Ok(Wire),
            "wor" => Ok(WOr),
            "string" => Ok(String),
            _ if gtkwave_extensions => match s {
                "port" => Ok(Port),
                "sparray" => Ok(SparseArray),
                "realtime" => Ok(RealTime),
                "bit" => Ok(Bit),
                "logic" => Ok(Logic),
                "int" => Ok(Int),
                "shortint" => Ok(ShortInt),
                "longint" => Ok(LongInt),
                "byte" => Ok(Byte),
                "enum" => Ok(Enum),
                "shortreal" => Ok(ShortReal),
                "real_parameter" => Ok(RealParameter),
                _ => Err(InvalidVarType),
            },
            _ => Err(InvalidVarType),
        }
    }
}

impl FromStr for VarType {
    type Err = InvalidVarType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        VarType::from_str_ext(s, true)
    }
}

impl Display for VarType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use VarType::*;
        write!(
            f,
            "{}",
            match *self {
                Event => "event",
                Integer => "integer",
                Parameter => "parameter",
                Real => "real",
                Reg => "reg",
                Supply0 => "supply0",
                Supply1 => "supply1",
                Time => "time",
                Tri => "tri",
                TriAnd => "triand",
                TriOr => "trior",
                TriReg => "trireg",
                Tri0 => "tri0",
                Tri1 => "tri1",
                WAnd => "wand",
                Wire => "wire",
                WOr => "wor",
                String => "string",
                Port => "port",
                SparseArray => "sparray",
                RealTime => "realtime",
                Bit => "bit",
                Logic => "logic",
                Int => "int",
                ShortInt => "shortint",
                LongInt => "longint",
                Byte => "byte",
                Enum => "enum",
                ShortReal => "shortreal",
                RealParameter => "real_parameter",
            }
        )
    }
}

/// Information on a VCD scope as represented by a `$scope` command and its children.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Scope {
    /// Type of scope.
    pub scope_type: ScopeType,

    /// Name of the scope.
    pub identifier: String,

    /// Items within the scope.
    pub items: Vec<ScopeItem>,
}

impl Scope {
    /// Create a `Scope`.
    pub fn new(scope_type: ScopeType, identifier: String) -> Self {
        Self { scope_type, identifier, items: Vec::new() }
    }

    /// Looks up a variable by reference.
    pub fn find_var<'a>(&'a self, reference: &str) -> Option<&'a Var> {
        for c in &self.items {
            if let ScopeItem::Var(v) = c {
                if v.reference == reference {
                    return Some(v);
                }
            }
        }
        None
    }
}

impl Default for Scope {
    fn default() -> Scope {
        Scope {
            scope_type: ScopeType::Module,
            identifier: "".to_string(),
            items: Vec::new(),
        }
    }
}

/// Index of a VCD variable reference: either a bit select index `[i]` or a range index `[msb:lsb]`
///
/// `ReferenceIndex` can be parsed with [`FromStr`]:
///
/// ```ignore
/// # use vcd::ReferenceIndex;
/// assert_eq!("[7:0]".parse(), Ok(ReferenceIndex::Range(7, 0)));
/// ```
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ReferenceIndex {
    /// Single bit (e.g `[0]`)
    BitSelect(i32),

    /// Range of bits (e.g. `[7:0]`)
    Range(i32, i32),
}

super::unit_error_struct!(InvalidReferenceIndex, "invalid reference index");

impl FromStr for ReferenceIndex {
    type Err = InvalidReferenceIndex;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix('[').ok_or(InvalidReferenceIndex)?;
        let s = s.strip_suffix(']').ok_or(InvalidReferenceIndex)?;
        match s.split_once(':') {
            Some((msb_str, lsb_str)) => {
                let msb: i32 = msb_str
                    .trim()
                    .parse()
                    .map_err(|_| InvalidReferenceIndex)?;
                let lsb: i32 = lsb_str
                    .trim()
                    .parse()
                    .map_err(|_| InvalidReferenceIndex)?;
                Ok(ReferenceIndex::Range(msb, lsb))
            }
            None => {
                let idx = s
                    .trim()
                    .parse()
                    .map_err(|_| InvalidReferenceIndex{})?;
                Ok(ReferenceIndex::BitSelect(idx))
            }
        }
    }
}

impl Display for ReferenceIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ReferenceIndex::*;
        match self {
            BitSelect(idx) => write!(f, "[{}]", idx),
            Range(msb, lsb) => write!(f, "[{}:{}]", msb, lsb),
        }
    }
}

#[test]
fn test_parse_reference_index() {
    assert_eq!("[0]".parse(), Ok(ReferenceIndex::BitSelect(0)));
    assert_eq!("[ 1 ]".parse(), Ok(ReferenceIndex::BitSelect(1)));
    assert_eq!("[7:0]".parse(), Ok(ReferenceIndex::Range(7, 0)));
    assert_eq!("[12:-4]".parse(), Ok(ReferenceIndex::Range(12, -4)));
    assert_eq!("[ 0 : 8 ]".parse(), Ok(ReferenceIndex::Range(0, 8)));
}

/// Information on a VCD variable as represented by a `$var` command.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Var {
    /// Type of variable.
    pub var_type: VarType,

    /// Width in bits.
    pub size: u32,

    /// Internal code used in value changes to link them back to this variable.
    ///
    /// Multiple variables can have the same `code` if they always have the same
    /// value.
    pub code: IdCode,

    /// Name of the variable.
    pub reference: String,

    /// Optional bit index or range associated with the `reference`.
    pub index: Option<ReferenceIndex>,
}

impl Var {
    /// Create a `Var`.
    pub fn new(
        var_type: VarType,
        size: u32,
        code: IdCode,
        reference: String,
        index: Option<ReferenceIndex>,
    ) -> Self {
        Self { var_type, size, code, reference, index }
    }
}

/// An item in a scope
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ScopeItem {
    /// `$scope` - Child scope
    Scope(Scope),

    /// `$var` - Variable
    Var(Var),

    /// `$comment` - Comment
    Comment(String),

    /// `$attrbegin` - GTKWave/FST attribute
    Attribute(Attribute),
}
