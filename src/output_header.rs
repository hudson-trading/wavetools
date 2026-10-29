// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT

//! Preserve source hierarchy order and GTKWave annotations when exporting VCD.

use crate::{
    vcd::{self, MiscAttributeSubtype},
    WaveHierarchy, WaveReader,
};
use fst_reader::{FstHierarchyEntry, FstScopeType};
use std::{collections::HashMap, io};

// FST supplemental variable attributes reserve the low 10 bits for the VHDL
// data type (FST_SUP_VAR_DATA_TYPE_BITS in fst-reader).
const FST_SUP_VAR_DATA_TYPE_BITS: u32 = 10;

fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn scope_type(t: FstScopeType) -> io::Result<vcd::ScopeType> {
    use FstScopeType::*;
    Ok(match t {
        Module => vcd::ScopeType::Module,
        Task => vcd::ScopeType::Task,
        Function => vcd::ScopeType::Function,
        Begin => vcd::ScopeType::Begin,
        Fork => vcd::ScopeType::Fork,
        Generate => vcd::ScopeType::Generate,
        Struct => vcd::ScopeType::Struct,
        Union => vcd::ScopeType::Union,
        Class => vcd::ScopeType::Class,
        Interface => vcd::ScopeType::Interface,
        Package => vcd::ScopeType::Package,
        Program => vcd::ScopeType::Program,
        VhdlArchitecture => vcd::ScopeType::VhdlArchitecture,
        VhdlProcedure => vcd::ScopeType::VhdlProcedure,
        VhdlFunction => vcd::ScopeType::VhdlFunction,
        VhdlRecord => vcd::ScopeType::VhdlRecord,
        VhdlProcess => vcd::ScopeType::VhdlProcess,
        VhdlBlock => vcd::ScopeType::VhdlBlock,
        VhdlForGenerate => vcd::ScopeType::VhdlForGenerate,
        VhdlIfGenerate => vcd::ScopeType::VhdlIfGenerate,
        VhdlGenerate => vcd::ScopeType::VhdlGenerate,
        VhdlPackage => vcd::ScopeType::VhdlPackage,
        SvArray => vcd::ScopeType::SvArray,
        _ => return Err(error(format!("Unsupported FST scope type {t:?}"))),
    })
}

fn attr(text: String) -> io::Result<vcd::Command> {
    // Reuse the canonical subtype spellings used by the hierarchy reader.
    let (kind, rest) = text
        .split_once(' ')
        .ok_or_else(|| error("Invalid attribute"))?;
    let (subtype, rest) = rest
        .split_once(": ")
        .ok_or_else(|| error("Invalid attribute subtype"))?;
    let (name, arg) = rest
        .rsplit_once(' ')
        .ok_or_else(|| error("Invalid attribute argument"))?;
    Ok(vcd::Command::AttributeBegin(
        kind.parse().map_err(error)?,
        subtype.into(),
        name.into(),
        arg.parse().map_err(error)?,
    ))
}

fn fst_command(entry: FstHierarchyEntry) -> io::Result<vcd::Command> {
    use vcd::{AttributeType::Misc, Command::*};
    Ok(match entry {
        FstHierarchyEntry::Scope { tpe, name, .. } => ScopeDef(scope_type(tpe)?, name),
        FstHierarchyEntry::UpScope => Upscope,
        FstHierarchyEntry::Var {
            tpe,
            name,
            length,
            handle,
            ..
        } => {
            let size = match tpe {
                fst_reader::FstVarType::Real
                | fst_reader::FstVarType::RealParameter
                | fst_reader::FstVarType::RealTime => crate::VCD_REAL_WIDTH_BITS,
                _ => length,
            };
            VarDef(
                crate::fst_var_type_str(tpe).parse().map_err(error)?,
                size,
                (handle.get_index() as u64).into(),
                name,
                None,
            )
        }
        FstHierarchyEntry::EnumTable {
            name,
            handle,
            mapping,
        } => {
            let values = mapping
                .iter()
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let encodings = mapping
                .iter()
                .map(|(encoding, _)| encoding.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            AttributeBegin(
                Misc,
                MiscAttributeSubtype::EnumTable.to_hex().into(),
                format!("{name} {} {values} {encodings}", mapping.len()),
                handle as i64,
            )
        }
        FstHierarchyEntry::EnumTableRef { handle } => AttributeBegin(
            Misc,
            MiscAttributeSubtype::EnumTable.to_hex().into(),
            "\"\"".into(),
            handle as i64,
        ),
        FstHierarchyEntry::Array {
            name,
            array_type,
            left,
            right,
        } => attr(crate::format_array_attr(&name, array_type, left, right))?,
        FstHierarchyEntry::Pack {
            name,
            pack_type,
            value,
        } => attr(crate::format_pack_attr(&name, pack_type, value))?,
        FstHierarchyEntry::SVEnum {
            name,
            enum_type,
            value,
        } => attr(crate::format_sv_enum_attr(&name, enum_type, value))?,
        FstHierarchyEntry::PathName { id, name } => AttributeBegin(
            Misc,
            MiscAttributeSubtype::PathName.to_hex().into(),
            name,
            id as i64,
        ),
        FstHierarchyEntry::SourceStem {
            is_instantiation,
            path_id,
            line,
        } => AttributeBegin(
            Misc,
            if is_instantiation {
                MiscAttributeSubtype::SourceInstantiationStem
            } else {
                MiscAttributeSubtype::SourceStem
            }
            .to_hex()
            .into(),
            path_id.to_string(),
            line as i64,
        ),
        FstHierarchyEntry::VhdlVarInfo {
            type_name,
            var_type,
            data_type,
        } => AttributeBegin(
            Misc,
            MiscAttributeSubtype::SupVar.to_hex().into(),
            type_name,
            ((var_type as i64) << FST_SUP_VAR_DATA_TYPE_BITS) | data_type as i64,
        ),
        FstHierarchyEntry::Comment { string } => Comment(string),
        FstHierarchyEntry::AttributeEnd => AttributeEnd,
    })
}

fn walk_vcd(
    items: &[vcd::ScopeItem],
    emit: &mut impl FnMut(vcd::Command) -> io::Result<()>,
) -> io::Result<()> {
    use vcd::{Command, ScopeItem};
    for item in items {
        match item {
            ScopeItem::Scope(scope) => {
                emit(Command::ScopeDef(
                    scope.scope_type,
                    scope.identifier.clone(),
                ))?;
                walk_vcd(&scope.items, emit)?;
                emit(Command::Upscope)?;
            }
            ScopeItem::Var(v) => emit(Command::VarDef(
                v.var_type,
                v.size,
                v.code,
                v.reference.clone(),
                v.index,
            ))?,
            ScopeItem::Comment(c) => emit(Command::Comment(c.clone()))?,
            ScopeItem::Attribute(a) => emit(Command::AttributeBegin(
                a.attr_type,
                a.subtype.clone(),
                a.name.clone(),
                a.arg,
            ))?,
            ScopeItem::AttributeEnd => emit(Command::AttributeEnd)?,
        }
    }
    Ok(())
}

/// Visit local handle IDs without retaining a second copy of the source header.
fn visit_commands(
    reader: &mut WaveReader,
    emit: &mut impl FnMut(vcd::Command) -> io::Result<()>,
) -> io::Result<()> {
    match reader {
        WaveReader::Vcd(r) => walk_vcd(&r.header.items, &mut |mut command| {
            if let vcd::Command::VarDef(_, _, code, _, _) = &mut command {
                *code = (r.id_to_idx[code] as u64).into();
            }
            emit(command)
        }),
        WaveReader::Fst(r) => {
            let mut result = Ok(());
            r.read_hierarchy(|entry| {
                if result.is_ok() {
                    result = fst_command(entry).and_then(&mut *emit);
                }
            })
            .map_err(error)?;
            result
        }
    }
}

/// Only annotations/comments need lookahead: a following scope determines
/// whether packing attributes belong to that scope or the preceding variable.
fn visit_annotated(
    reader: &mut WaveReader,
    mut emit: impl FnMut(vcd::Command, bool) -> io::Result<()>,
) -> io::Result<()> {
    use vcd::{AttributeType, Command::*};
    let mut pending = Vec::new();
    visit_commands(reader, &mut |command| {
        if matches!(command, AttributeBegin(..) | Comment(_)) {
            pending.push(command);
            return Ok(());
        }
        let next_is_scope = matches!(command, ScopeDef(..));
        for attribute in pending.drain(..) {
            let scope_attribute = next_is_scope
                && matches!(
                    attribute,
                    AttributeBegin(
                        AttributeType::Pack | AttributeType::Array | AttributeType::Enum,
                        ..
                    )
                );
            emit(attribute, scope_attribute)?;
        }
        emit(command, false)
    })?;
    for command in pending {
        emit(command, false)?;
    }
    Ok(())
}

pub(crate) fn write_header<W: io::Write>(
    writer: &mut vcd::Writer<W>,
    readers: &mut [WaveReader],
    offsets: &[usize],
    hierarchy: &WaveHierarchy,
) -> io::Result<()> {
    let selected: HashMap<_, _> = hierarchy
        .signal_map
        .iter()
        .flat_map(|(&handle, info)| info.vars.iter().map(move |var| ((handle, var.name), var)))
        .collect();
    // Enum and source-path ids are local to each input; remap them for overlays.
    let mut enum_ids = HashMap::new();
    let mut path_ids = HashMap::new();
    fn variable_count(items: &[vcd::ScopeItem]) -> usize {
        items
            .iter()
            .map(|item| match item {
                vcd::ScopeItem::Var(_) => 1,
                vcd::ScopeItem::Scope(scope) => variable_count(&scope.items),
                _ => 0,
            })
            .sum()
    }
    let definitions_needed = readers.len() != 1
        || match &readers[0] {
            WaveReader::Fst(r) => r.get_header().var_count as usize != selected.len(),
            WaveReader::Vcd(r) => variable_count(&r.header.items) != selected.len(),
        };
    // Shared definitions must precede declarations. The first pass emits only
    // those definitions; the second streams the remaining commands. ID maps are
    // populated in source order on the first pass and reused on the second.
    for definitions_only in [true, false] {
        if definitions_only && !definitions_needed {
            continue;
        }
        let overlay = readers.len() > 1;
        for (file, (reader, &offset)) in readers.iter_mut().zip(offsets).enumerate() {
            let mut scopes = vec![Some(hierarchy.names.root())];
            let mut in_preamble = true;
            // Variable attributes follow their declaration; scope annotations and
            // shared definitions must survive removal of the preceding variable.
            let mut selected_variable = None;
            let mut emit = |mut command: vcd::Command, scope_attribute: bool| -> io::Result<()> {
                use vcd::Command::*;
                let definition = matches!(&command,
                AttributeBegin(vcd::AttributeType::Misc, subtype, name, _)
                if subtype == MiscAttributeSubtype::PathName.to_hex()
                    || (subtype == MiscAttributeSubtype::EnumTable.to_hex() && !name.trim_matches('"').is_empty()));
                let attribute = matches!(&command, AttributeBegin(..) | AttributeEnd);
                let hoist = (overlay && in_preamble && attribute)
                    || (definition && selected_variable == Some(false));
                if attribute && selected_variable == Some(false) && !hoist && !scope_attribute {
                    return Ok(());
                }
                match &mut command {
                    ScopeDef(_, name) => {
                        let scope = scopes
                            .last()
                            .copied()
                            .flatten()
                            .and_then(|parent| hierarchy.names.child(parent, name));
                        scopes.push(scope);
                        selected_variable = None;
                        in_preamble = false;
                    }
                    Upscope => {
                        scopes.pop();
                        selected_variable = None;
                    }
                    VarDef(_, _, code, name, index) => {
                        in_preamble = false;
                        let local = u64::from(*code) as usize;
                        let handle = offset + local;
                        let reference = match index {
                            Some(index) => std::borrow::Cow::Owned(format!("{name} {index}")),
                            None => std::borrow::Cow::Borrowed(name.as_str()),
                        };
                        let var = scopes.last().copied().flatten().and_then(|parent| {
                            let id = hierarchy.names.child(parent, &reference).or_else(|| {
                                hierarchy.names.child(parent, &reference.replace(" [", "["))
                            })?;
                            selected.get(&(handle, id))
                        });
                        selected_variable = Some(var.is_some());
                        let Some(var) = var else { return Ok(()) };
                        *code = (handle as u64).into();
                        let output_name = hierarchy.names.segment(var.name);
                        if name != output_name {
                            *name = output_name.to_string();
                        }
                        *index = None;
                    }
                    AttributeBegin(vcd::AttributeType::Misc, subtype, name, arg) => {
                        let subtype = MiscAttributeSubtype::from_hex(subtype).ok();
                        let ids = match subtype {
                            Some(MiscAttributeSubtype::EnumTable) => Some(&mut enum_ids),
                            Some(MiscAttributeSubtype::PathName) => Some(&mut path_ids),
                            _ => None,
                        };
                        if let Some(ids) = ids {
                            let next = ids.len() as i64 + 1;
                            *arg = *ids.entry((file, *arg)).or_insert(next);
                        } else if matches!(
                            subtype,
                            Some(
                                MiscAttributeSubtype::SourceStem
                                    | MiscAttributeSubtype::SourceInstantiationStem
                            )
                        ) {
                            let old = name.parse::<i64>().map_err(error)?;
                            let next = path_ids.len() as i64 + 1;
                            *name = path_ids.entry((file, old)).or_insert(next).to_string();
                        }
                    }
                    _ => {}
                }
                if hoist == definitions_only {
                    writer.command(&command)?;
                }
                Ok(())
            };
            visit_annotated(reader, &mut emit)?;
        }
    }
    writer.enddefinitions()
}
