import { buildBdlIr } from "@disjukr/bdl/ir/builder";
import type * as bdl from "@disjukr/bdl/ir";

interface Schema {
  declarations: Declaration[];
}

type Declaration =
  | ProcDeclaration
  | StructDeclaration
  | EnumDeclaration
  | UnionDeclaration;

interface ProcDeclaration {
  kind: "proc";
  name: string;
  id: number;
  stream: ProcStream;
  input: TypeRef;
  output: TypeRef;
  error: TypeRef;
}

type ProcStream = "unary" | "server" | "client" | "bidi";

interface StructDeclaration {
  kind: "struct";
  name: string;
  typePath: string;
  fields: FieldDeclaration[];
}

interface EnumDeclaration {
  kind: "enum";
  name: string;
  typePath: string;
  variants: EnumVariantDeclaration[];
}

interface UnionDeclaration {
  kind: "union";
  name: string;
  typePath: string;
  variants: UnionVariantDeclaration[];
}

interface EnumVariantDeclaration {
  id: number;
  name: string;
}

interface UnionVariantDeclaration {
  id: number;
  name: string;
  fields: FieldDeclaration[];
}

interface FieldDeclaration {
  id: number;
  name: string;
  optional: boolean;
  type: TypeRef;
}

type TypeRef =
  | { kind: "primitive"; name: PrimitiveTypeName }
  | { kind: "named"; path: string }
  | { kind: "array"; item: TypeRef }
  | { kind: "void" };

type PrimitiveTypeName = "u53" | "i53" | "string" | "bool" | "bytes";

interface CliOptions {
  byteCodec: boolean;
  out?: string;
  schemaRoots: string[];
}

const primitiveTypes = new Set(["u53", "i53", "string", "bool", "bytes"]);
const rustKeywords = new Set([
  "as",
  "break",
  "const",
  "continue",
  "crate",
  "else",
  "enum",
  "extern",
  "false",
  "fn",
  "for",
  "if",
  "impl",
  "in",
  "let",
  "loop",
  "match",
  "mod",
  "move",
  "mut",
  "pub",
  "ref",
  "return",
  "self",
  "Self",
  "static",
  "struct",
  "super",
  "trait",
  "true",
  "type",
  "unsafe",
  "use",
  "where",
  "while",
  "async",
  "await",
  "dyn",
]);

function parseCli(args: string[]): CliOptions {
  const options: CliOptions = { byteCodec: true, schemaRoots: [] };
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--out") {
      options.out = requiredArg(args, ++i, arg);
    } else if (arg === "--no-byte-codec") {
      options.byteCodec = false;
    } else if (arg === "--schema") {
      options.schemaRoots.push(requiredArg(args, ++i, arg));
    } else if (arg === "--help" || arg === "-h") {
      printHelpAndExit();
    } else if (arg.startsWith("-")) {
      throw new Error(`unknown option ${arg}`);
    } else {
      options.schemaRoots.push(arg);
    }
  }
  if (options.schemaRoots.length === 0) {
    throw new Error("at least one --schema is required");
  }
  return options;
}

function requiredArg(args: string[], index: number, option: string): string {
  const value = args[index];
  if (!value) throw new Error(`${option} requires a value`);
  return value;
}

function printHelpAndExit(): never {
  console.log(`Usage: deno run -A scripts/codegen/rust/main.ts [options]

Options:
  --schema <path> BDL file or directory.
  --out <path>    Generated Rust output file. Defaults to stdout.
  --no-byte-codec Do not emit encode/decode byte wrappers.
`);
  Deno.exit(0);
}

async function loadSchema(schemaRoots: string[]): Promise<Schema> {
  const files: string[] = [];
  for (const root of schemaRoots) files.push(...await collectBdlFiles(root));
  files.sort((a, b) => a.localeCompare(b));

  const { ir } = await buildBdlIr({
    entryModulePaths: files,
    resolveModuleFile: async (modulePath) => ({
      fileUrl: toFileUrl(modulePath),
      text: await Deno.readTextFile(modulePath),
    }),
  });
  return schemaFromIr(ir);
}

async function collectBdlFiles(path: string): Promise<string[]> {
  const stat = await Deno.stat(path);
  if (stat.isFile) return path.endsWith(".bdl") ? [normalizePath(path)] : [];
  if (!stat.isDirectory) return [];

  const files: string[] = [];
  for await (const entry of Deno.readDir(path)) {
    const child = `${normalizePath(path)}/${entry.name}`;
    if (entry.isDirectory) files.push(...await collectBdlFiles(child));
    else if (entry.isFile && child.endsWith(".bdl")) files.push(child);
  }
  return files;
}

function schemaFromIr(ir: bdl.BdlIr): Schema {
  const declarations: Declaration[] = [];
  const typePaths = new Map<string, string>();

  for (const [typePath, def] of Object.entries(ir.defs)) {
    if (def.type === "Proc") continue;
    if (typePaths.has(def.name)) {
      throw new Error(`duplicate Rust declaration name ${def.name}`);
    }
    typePaths.set(def.name, typePath);
  }

  for (const [typePath, def] of Object.entries(ir.defs)) {
    if (def.type === "Proc") {
      declarations.push({
        kind: "proc",
        name: def.name,
        id: requiredId(def, typePath),
        stream: requiredProcStream(def, typePath),
        input: typeRefFromIr(def.inputType),
        output: typeRefFromIr(def.outputType),
        error: typeRefFromIr(def.errorType ?? plainType("void")),
      });
    } else if (def.type === "Struct") {
      declarations.push({
        kind: "struct",
        name: def.name,
        typePath,
        fields: def.fields.map((field) =>
          fieldFromIr(field, `${typePath}.${field.name}`)
        ),
      });
    } else if (def.type === "Enum") {
      declarations.push({
        kind: "enum",
        name: def.name,
        typePath,
        variants: def.items.map((item) => ({
          id: requiredId(item, `${typePath}.${item.name}`),
          name: item.name,
        })),
      });
    } else if (def.type === "Union") {
      declarations.push({
        kind: "union",
        name: def.name,
        typePath,
        variants: def.items.map((item) => ({
          id: requiredId(item, `${typePath}.${item.name}`),
          name: item.name,
          fields: item.fields.map((field) =>
            fieldFromIr(field, `${typePath}.${item.name}.${field.name}`)
          ),
        })),
      });
    }
  }

  validateSchema({ declarations });
  return { declarations };
}

function fieldFromIr(field: bdl.StructField, label: string): FieldDeclaration {
  return {
    id: requiredId(field, label),
    name: field.name,
    optional: field.optional,
    type: typeRefFromIr(field.fieldType),
  };
}

function typeRefFromIr(type: bdl.Type): TypeRef {
  if (type.type === "Array") {
    return {
      kind: "array",
      item: typeRefFromIr(plainType(type.valueTypePath)),
    };
  }
  if (type.type === "Dictionary") {
    throw new Error("Rust codegen does not support dictionary fields yet");
  }
  if (type.valueTypePath === "void") return { kind: "void" };
  if (primitiveTypes.has(type.valueTypePath)) {
    return { kind: "primitive", name: type.valueTypePath as PrimitiveTypeName };
  }
  return { kind: "named", path: type.valueTypePath };
}

function plainType(valueTypePath: string): bdl.Plain {
  return { type: "Plain", valueTypePath };
}

function validateSchema(schema: Schema) {
  const names = new Set<string>();
  const paths = new Set<string>();
  const procIds = new Set<number>();
  for (const declaration of schema.declarations) {
    if (declaration.kind === "proc") {
      if (names.has(declaration.name)) {
        throw new Error(`duplicate declaration ${declaration.name}`);
      }
      names.add(declaration.name);
      if (procIds.has(declaration.id)) {
        throw new Error(`duplicate proc id ${declaration.id}`);
      }
      procIds.add(declaration.id);
      continue;
    }
    if (names.has(declaration.name)) {
      throw new Error(`duplicate declaration ${declaration.name}`);
    }
    names.add(declaration.name);
    paths.add(declaration.typePath);
  }
  for (const declaration of schema.declarations) {
    if (declaration.kind === "proc") {
      validateTypeRef(paths, declaration.input);
      validateTypeRef(paths, declaration.output);
      validateTypeRef(paths, declaration.error);
    } else if (declaration.kind === "struct") {
      validateUniqueIds(`${declaration.name} fields`, declaration.fields);
      for (const field of declaration.fields) {
        validateTypeRef(paths, field.type);
      }
    } else if (declaration.kind === "enum") {
      validateUniqueIds(`${declaration.name} variants`, declaration.variants);
    } else {
      validateUniqueIds(`${declaration.name} variants`, declaration.variants);
      for (const variant of declaration.variants) {
        validateUniqueIds(
          `${declaration.name}.${variant.name} fields`,
          variant.fields,
        );
        for (const field of variant.fields) validateTypeRef(paths, field.type);
      }
    }
  }
}

function validateTypeRef(paths: Set<string>, type: TypeRef) {
  if (type.kind === "array") validateTypeRef(paths, type.item);
  if (type.kind === "named" && !paths.has(type.path)) {
    throw new Error(`unknown type ${type.path}`);
  }
}

function validateUniqueIds(
  label: string,
  items: Array<{ id: number; name: string }>,
) {
  const ids = new Set<number>();
  for (const item of items) {
    if (ids.has(item.id)) {
      throw new Error(`duplicate id ${item.id} in ${label}`);
    }
    ids.add(item.id);
  }
}

function emitRust(schema: Schema, options: CliOptions): string {
  const out = new Writer();
  const named = declarationMap(schema);
  out.line("// Generated by scripts/codegen/rust/main.ts");
  out.line("// Do not edit by hand.");
  out.line("use std::collections::BTreeMap;");
  out.line();
  out.line("use crate::cbor::{CborError, Value};");
  out.line();
  emitCodecError(out);
  for (const declaration of schema.declarations) {
    if (declaration.kind === "proc") continue;
    out.line();
    emitTypeDeclaration(out, declaration, named);
  }
  for (const declaration of schema.declarations) {
    if (declaration.kind === "proc") continue;
    out.line();
    emitCodec(out, declaration, named, options);
  }
  emitProcHelpers(out, schema, named, options);
  emitRuntimeHelpers(out);
  return out.toString();
}

function emitCodecError(out: Writer) {
  out.line("#[derive(Debug, Clone, PartialEq)]");
  out.line("pub enum CodecError {");
  out.indent(() => {
    out.line("Cbor(CborError),");
    out.line("ExpectedArray,");
    out.line("ExpectedBool,");
    out.line("ExpectedBytes,");
    out.line("ExpectedInteger,");
    out.line("ExpectedMap,");
    out.line("ExpectedText,");
    out.line("IntegerOutOfRange(&'static str),");
    out.line(
      "UnknownEnumVariant { type_name: &'static str, variant_id: u64 },",
    );
    out.line(
      "UnknownUnionVariant { type_name: &'static str, variant_id: u64 },",
    );
  });
  out.line("}");
  out.line();
  out.line("impl From<CborError> for CodecError {");
  out.indent(() => {
    out.line("fn from(value: CborError) -> Self {");
    out.indent(() => out.line("Self::Cbor(value)"));
    out.line("}");
  });
  out.line("}");
  out.line();
  out.line("impl std::fmt::Display for CodecError {");
  out.indent(() => {
    out.line(
      "fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {",
    );
    out.indent(() => {
      out.line("match self {");
      out.indent(() => {
        out.line("Self::Cbor(error) => write!(formatter, \"cbor error: {error}\"),");
        out.line("Self::ExpectedArray => formatter.write_str(\"expected CBOR array\"),");
        out.line("Self::ExpectedBool => formatter.write_str(\"expected CBOR bool\"),");
        out.line("Self::ExpectedBytes => formatter.write_str(\"expected CBOR bytes\"),");
        out.line("Self::ExpectedInteger => formatter.write_str(\"expected CBOR integer\"),");
        out.line("Self::ExpectedMap => formatter.write_str(\"expected CBOR map\"),");
        out.line("Self::ExpectedText => formatter.write_str(\"expected CBOR text\"),");
        out.line("Self::IntegerOutOfRange(type_name) => write!(formatter, \"integer is out of range for {type_name}\"),");
        out.line("Self::UnknownEnumVariant { type_name, variant_id } => write!(formatter, \"unknown {type_name} enum variant {variant_id}\"),");
        out.line("Self::UnknownUnionVariant { type_name, variant_id } => write!(formatter, \"unknown {type_name} union variant {variant_id}\"),");
      });
      out.line("}");
    });
    out.line("}");
  });
  out.line("}");
  out.line();
  out.line("impl std::error::Error for CodecError {}");
}

function emitTypeDeclaration(
  out: Writer,
  declaration: Exclude<Declaration, ProcDeclaration>,
  named: Map<string, Declaration>,
) {
  if (declaration.kind === "enum") {
    out.line("#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
  } else {
    out.line("#[derive(Debug, Clone, PartialEq, Eq)]");
  }
  if (declaration.kind === "struct") {
    out.line(`pub struct ${declaration.name} {`);
    out.indent(() => {
      for (const field of declaration.fields) {
        out.line(`pub ${fieldName(field.name)}: ${rustFieldType(field)},`);
      }
    });
    out.line("}");
  } else if (declaration.kind === "enum") {
    out.line(`pub enum ${declaration.name} {`);
    out.indent(() => {
      for (const variant of declaration.variants) {
        out.line(`${variant.name},`);
      }
    });
    out.line("}");
  } else {
    out.line(`pub enum ${declaration.name} {`);
    out.indent(() => {
      for (const variant of declaration.variants) {
        if (variant.fields.length === 0) {
          out.line(`${variant.name},`);
        } else {
          out.line(`${variant.name} {`);
          out.indent(() => {
            for (const field of variant.fields) {
              out.line(
                `${fieldName(field.name)}: ${rustFieldType(field)},`,
              );
            }
          });
          out.line("},");
        }
      }
    });
    out.line("}");
  }

  if (declaration.kind === "struct" || declaration.kind === "union") {
    emitDefaultImpl(out, declaration, named);
  }
}

function rustFieldType(field: FieldDeclaration): string {
  const type = rustType(field.type);
  return field.optional ? `Option<${type}>` : type;
}

function emitDefaultImpl(
  out: Writer,
  declaration: StructDeclaration | UnionDeclaration,
  named: Map<string, Declaration>,
) {
  out.line();
  out.line(`impl Default for ${declaration.name} {`);
  out.indent(() => {
    out.line("fn default() -> Self {");
    out.indent(() => {
      if (declaration.kind === "struct") {
        out.line("Self {");
        out.indent(() => {
          for (const field of declaration.fields) {
            out.line(
              `${fieldName(field.name)}: ${defaultForField(field, named)},`,
            );
          }
        });
        out.line("}");
      } else {
        const variant = declaration.variants[0]!;
        if (variant.fields.length === 0) {
          out.line(`Self::${variant.name}`);
        } else {
          out.line(`Self::${variant.name} {`);
          out.indent(() => {
            for (const field of variant.fields) {
              out.line(
                `${fieldName(field.name)}: ${defaultForField(field, named)},`,
              );
            }
          });
          out.line("}");
        }
      }
    });
    out.line("}");
  });
  out.line("}");
}

function emitCodec(
  out: Writer,
  declaration: Exclude<Declaration, ProcDeclaration>,
  named: Map<string, Declaration>,
  options: CliOptions,
) {
  if (declaration.kind === "struct") {
    emitStructCodec(out, declaration, named, options);
  } else if (declaration.kind === "enum") {
    emitEnumCodec(out, declaration, options);
  } else {
    emitUnionCodec(out, declaration, named, options);
  }
}

function emitStructCodec(
  out: Writer,
  declaration: StructDeclaration,
  named: Map<string, Declaration>,
  options: CliOptions,
) {
  out.line(`impl ${declaration.name} {`);
  out.indent(() => {
    if (options.byteCodec) emitEncodeDecodeWrappers(out, declaration.name);
    out.line("pub fn encode_value(&self) -> Result<Value, CodecError> {");
    out.indent(() => {
      out.line("let mut fields = BTreeMap::new();");
      for (const field of declaration.fields) {
        emitEncodeField(out, field, named);
      }
      out.line("Ok(Value::Map(fields))");
    });
    out.line("}");
    out.line();
    out.line(
      "pub fn decode_value(value: &Value) -> Result<Self, CodecError> {",
    );
    out.indent(() => {
      out.line("let fields = expect_map(value)?;");
      out.line("Ok(Self {");
      out.indent(() => {
        for (const field of declaration.fields) {
          out.line(
            `${fieldName(field.name)}: ${decodeFieldExpr(field, named)}?,`,
          );
        }
      });
      out.line("})");
    });
    out.line("}");
  });
  out.line("}");
}

function emitEnumCodec(
  out: Writer,
  declaration: EnumDeclaration,
  options: CliOptions,
) {
  out.line(`impl ${declaration.name} {`);
  out.indent(() => {
    if (options.byteCodec) emitEncodeDecodeWrappers(out, declaration.name);
    out.line("pub fn encode_value(&self) -> Result<Value, CodecError> {");
    out.indent(() => {
      out.line("Ok(Value::U64(match self {");
      out.indent(() => {
        for (const variant of declaration.variants) {
          out.line(`Self::${variant.name} => ${variant.id},`);
        }
      });
      out.line("}))");
    });
    out.line("}");
    out.line();
    out.line(
      "pub fn decode_value(value: &Value) -> Result<Self, CodecError> {",
    );
    out.indent(() => {
      out.line("match expect_u64(value)? {");
      out.indent(() => {
        for (const variant of declaration.variants) {
          out.line(`${variant.id} => Ok(Self::${variant.name}),`);
        }
        out.line("variant_id => Err(CodecError::UnknownEnumVariant {");
        out.indent(() => {
          out.line(`type_name: "${declaration.name}",`);
          out.line("variant_id,");
        });
        out.line("}),");
      });
      out.line("}");
    });
    out.line("}");
  });
  out.line("}");
}

function emitUnionCodec(
  out: Writer,
  declaration: UnionDeclaration,
  named: Map<string, Declaration>,
  options: CliOptions,
) {
  out.line(`impl ${declaration.name} {`);
  out.indent(() => {
    if (options.byteCodec) emitEncodeDecodeWrappers(out, declaration.name);
    out.line("pub fn encode_value(&self) -> Result<Value, CodecError> {");
    out.indent(() => {
      out.line("match self {");
      out.indent(() => {
        for (const variant of declaration.variants) {
          emitUnionEncodeArm(out, declaration, variant, named);
        }
      });
      out.line("}");
    });
    out.line("}");
    out.line();
    out.line(
      "pub fn decode_value(value: &Value) -> Result<Self, CodecError> {",
    );
    out.indent(() => {
      out.line("let (variant_id, fields) = expect_union(value)?;");
      out.line("match variant_id {");
      out.indent(() => {
        for (const variant of declaration.variants) {
          emitUnionDecodeArm(out, declaration, variant, named);
        }
        out.line("variant_id => Err(CodecError::UnknownUnionVariant {");
        out.indent(() => {
          out.line(`type_name: "${declaration.name}",`);
          out.line("variant_id,");
        });
        out.line("}),");
      });
      out.line("}");
    });
    out.line("}");
  });
  out.line("}");
}

function emitEncodeDecodeWrappers(out: Writer, name: string) {
  out.line("pub fn try_encode(&self) -> Result<Vec<u8>, CodecError> {");
  out.indent(() => out.line("Ok(self.encode_value()?.encode())"));
  out.line("}");
  out.line();
  out.line("pub fn encode(&self) -> Vec<u8> {");
  out.indent(() =>
    out.line(
      `self.try_encode().expect("generated ${name} model failed to encode")`,
    )
  );
  out.line("}");
  out.line();
  out.line("pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {");
  out.indent(() => out.line("Self::decode_value(&Value::decode(bytes)?)"));
  out.line("}");
  out.line();
  out.line(`pub fn type_name() -> &'static str { "${name}" }`);
  out.line();
}

function emitProcHelpers(
  out: Writer,
  schema: Schema,
  named: Map<string, Declaration>,
  options: CliOptions,
) {
  const procs = procDeclarations(schema);
  if (procs.length === 0 || !options.byteCodec) return;

  out.line();
  out.line("#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
  out.line("pub enum ProcStream {");
  out.indent(() => {
    out.line("Unary,");
    out.line("Server,");
    out.line("Client,");
    out.line("Bidi,");
  });
  out.line("}");

  out.line();
  out.line("#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
  out.line("pub struct ProcDefinition {");
  out.indent(() => {
    out.line("pub id: ProcId,");
    out.line("pub wire_id: u64,");
    out.line("pub name: &'static str,");
    out.line("pub stream: ProcStream,");
  });
  out.line("}");

  out.line();
  out.line("#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
  out.line("pub enum ProcId {");
  out.indent(() => {
    for (const proc of procs) out.line(`${proc.name},`);
  });
  out.line("}");

  out.line();
  out.line(`pub static PROC_DEFINITIONS: [ProcDefinition; ${procs.length}] = [`);
  out.indent(() => {
    for (const proc of procs) {
      out.line("ProcDefinition {");
      out.indent(() => {
        out.line(`id: ProcId::${proc.name},`);
        out.line(`wire_id: ${proc.id},`);
        out.line(`name: "${proc.name}",`);
        out.line(`stream: ProcStream::${procStreamVariant(proc.stream)},`);
      });
      out.line("},");
    }
  });
  out.line("];");

  out.line();
  out.line("impl ProcId {");
  out.indent(() => {
    out.line(`pub const KNOWN: [Self; ${procs.length}] = [`);
    out.indent(() => {
      for (const proc of procs) out.line(`Self::${proc.name},`);
    });
    out.line("];");
    out.line();
    out.line("pub fn as_u64(self) -> u64 {");
    out.indent(() => out.line("self.definition().wire_id"));
    out.line("}");
    out.line();
    out.line("pub fn from_u64(value: u64) -> Option<Self> {");
    out.indent(() => {
      out.line("match value {");
      out.indent(() => {
        for (const proc of procs) out.line(`${proc.id} => Some(Self::${proc.name}),`);
        out.line("_ => None,");
      });
      out.line("}");
    });
    out.line("}");
    out.line();
    out.line("pub fn stream(self) -> ProcStream {");
    out.indent(() => out.line("self.definition().stream"));
    out.line("}");
    out.line();
    out.line("pub fn name(self) -> &'static str {");
    out.indent(() => out.line("self.definition().name"));
    out.line("}");
    out.line();
    out.line("pub fn definition(self) -> &'static ProcDefinition {");
    out.indent(() => {
      out.line("match self {");
      out.indent(() => {
        for (let index = 0; index < procs.length; index++) {
          const proc = procs[index]!;
          out.line(`Self::${proc.name} => &PROC_DEFINITIONS[${index}],`);
        }
      });
      out.line("}");
    });
    out.line("}");
  });
  out.line("}");

  out.line();
  out.line("#[derive(Debug, Clone, PartialEq, Eq)]");
  out.line("pub enum RpcRequest {");
  out.indent(() => {
    for (const proc of procs) emitProcMessageVariant(out, proc, proc.input);
  });
  out.line("}");

  out.line();
  out.line("#[derive(Debug, Clone, PartialEq)]");
  out.line("pub enum RpcRequestDecodeError {");
  out.indent(() => {
    out.line("UnknownProcId(u64),");
    out.line("MissingPayload { proc: ProcId },");
    out.line("MalformedPayload { proc: ProcId, source: CodecError },");
  });
  out.line("}");

  out.line();
  out.line("impl RpcRequest {");
  out.indent(() => {
    out.line(
      "pub fn decode(proc_id: u64, payload: Option<&[u8]>) -> Result<Self, RpcRequestDecodeError> {",
    );
    out.indent(() => {
      out.line("let proc = ProcId::from_u64(proc_id).ok_or(RpcRequestDecodeError::UnknownProcId(proc_id))?;");
      out.line("match proc {");
      out.indent(() => {
        for (const proc of procs) emitRpcRequestDecodeArm(out, proc);
      });
      out.line("}");
    });
    out.line("}");
    out.line();
    out.line("pub fn proc_id(&self) -> ProcId {");
    out.indent(() => {
      out.line("match self {");
      out.indent(() => {
        for (const proc of procs) emitProcMessageProcIdArm(out, "RpcRequest", proc, proc.input);
      });
      out.line("}");
    });
    out.line("}");
    out.line();
    out.line("pub fn proc_name(&self) -> &'static str {");
    out.indent(() => out.line("self.proc_id().name()"));
    out.line("}");
  });
  out.line("}");

  out.line();
  out.line("#[derive(Debug, Clone, PartialEq, Eq)]");
  out.line("pub enum RpcResponse {");
  out.indent(() => {
    for (const proc of procs) emitProcMessageVariant(out, proc, proc.output);
  });
  out.line("}");

  out.line();
  out.line("impl RpcResponse {");
  out.indent(() => {
    out.line("pub fn proc_id(&self) -> ProcId {");
    out.indent(() => {
      out.line("match self {");
      out.indent(() => {
        for (const proc of procs) emitProcMessageProcIdArm(out, "RpcResponse", proc, proc.output);
      });
      out.line("}");
    });
    out.line("}");
    out.line();
    out.line("pub fn encode_payload(&self) -> Option<Vec<u8>> {");
    out.indent(() => {
      out.line("match self {");
      out.indent(() => {
        for (const proc of procs) emitRpcResponsePayloadArm(out, proc);
      });
      out.line("}");
    });
    out.line("}");
  });
  out.line("}");

}

function procDeclarations(schema: Schema): ProcDeclaration[] {
  return schema.declarations
    .filter((declaration): declaration is ProcDeclaration =>
      declaration.kind === "proc"
    )
    .toSorted((a, b) => a.id - b.id);
}

function emitProcMessageVariant(
  out: Writer,
  proc: ProcDeclaration,
  type: TypeRef,
) {
  if (type.kind === "void") {
    out.line(`${proc.name},`);
  } else {
    out.line(`${proc.name}(${rustType(type)}),`);
  }
}

function emitRpcRequestDecodeArm(out: Writer, proc: ProcDeclaration) {
  if (proc.input.kind === "void") {
    out.line(`ProcId::${proc.name} => Ok(Self::${proc.name}),`);
    return;
  }
  out.line(`ProcId::${proc.name} => {`);
  out.indent(() => {
    out.line("let Some(payload) = payload else {");
    out.indent(() =>
      out.line("return Err(RpcRequestDecodeError::MissingPayload { proc });")
    );
    out.line("};");
    out.line(
      `let value = ${typeName(proc.input)}::decode(payload).map_err(|source| RpcRequestDecodeError::MalformedPayload { proc, source })?;`,
    );
    out.line(`Ok(Self::${proc.name}(value))`);
  });
  out.line("},");
}

function emitProcMessageProcIdArm(
  out: Writer,
  enumName: string,
  proc: ProcDeclaration,
  type: TypeRef,
) {
  if (type.kind === "void") {
    out.line(`${enumName}::${proc.name} => ProcId::${proc.name},`);
  } else {
    out.line(`${enumName}::${proc.name}(..) => ProcId::${proc.name},`);
  }
}

function emitRpcResponsePayloadArm(out: Writer, proc: ProcDeclaration) {
  if (proc.output.kind === "void") {
    out.line(`Self::${proc.name} => None,`);
  } else {
    out.line(`Self::${proc.name}(value) => Some(value.encode()),`);
  }
}

function procStreamVariant(stream: ProcStream): string {
  if (stream === "unary") return "Unary";
  if (stream === "server") return "Server";
  if (stream === "client") return "Client";
  return "Bidi";
}

function emitEncodeField(
  out: Writer,
  field: FieldDeclaration,
  named: Map<string, Declaration>,
) {
  const name = fieldName(field.name);
  if (field.optional) {
    out.line(`if let Some(value) = &self.${name} {`);
    out.indent(() => {
      out.line(
        `fields.insert(${field.id}, ${
          encodeExpr("value", field.type, named)
        }?);`,
      );
    });
    out.line("}");
  } else {
    out.line(
      `fields.insert(${field.id}, ${
        encodeExpr(`&self.${name}`, field.type, named)
      }?);`,
    );
  }
}

function emitUnionEncodeArm(
  out: Writer,
  declaration: UnionDeclaration,
  variant: UnionVariantDeclaration,
  named: Map<string, Declaration>,
) {
  if (variant.fields.length === 0) {
    out.line(`Self::${variant.name} => {`);
  } else {
    out.line(`Self::${variant.name} {`);
    out.indent(() => {
      for (const field of variant.fields) out.line(`${fieldName(field.name)},`);
    });
    out.line("} => {");
  }
  out.indent(() => {
    out.line("let mut fields = BTreeMap::new();");
    for (const field of variant.fields) {
      const name = fieldName(field.name);
      if (field.optional) {
        out.line(`if let Some(value) = ${name} {`);
        out.indent(() => {
          out.line(
            `fields.insert(${field.id}, ${
              encodeExpr("value", field.type, named)
            }?);`,
          );
        });
        out.line("}");
      } else {
        out.line(
          `fields.insert(${field.id}, ${
            encodeExpr(name, field.type, named)
          }?);`,
        );
      }
    }
    out.line(
      `Ok(Value::Array(vec![Value::U64(${variant.id}), Value::Map(fields)]))`,
    );
  });
  out.line("}");
}

function emitUnionDecodeArm(
  out: Writer,
  declaration: UnionDeclaration,
  variant: UnionVariantDeclaration,
  named: Map<string, Declaration>,
) {
  out.line(
    `${variant.id} => Ok(Self::${variant.name}${
      variant.fields.length === 0 ? "" : " {"
    }`,
  );
  if (variant.fields.length > 0) {
    out.indent(() => {
      for (const field of variant.fields) {
        out.line(
          `${fieldName(field.name)}: ${decodeFieldExpr(field, named)}?,`,
        );
      }
    });
    out.line("}),");
  } else {
    out.line("),");
  }
}

function decodeFieldExpr(
  field: FieldDeclaration,
  named: Map<string, Declaration>,
): string {
  const access = `fields.get(&${field.id})`;
  if (field.optional) {
    return `optional_field(${access}, |value| ${
      decodeExpr("value", field.type, named)
    })`;
  }
  return `field_or_default(${access}, |value| ${
    decodeExpr("value", field.type, named)
  }, || ${defaultExpr(field.type, named)})`;
}

function rustType(type: TypeRef): string {
  if (type.kind === "void") return "()";
  if (type.kind === "array") return `Vec<${rustType(type.item)}>`;
  if (type.kind === "primitive") {
    if (type.name === "u53") return "u64";
    if (type.name === "i53") return "i64";
    if (type.name === "string") return "String";
    if (type.name === "bool") return "bool";
    if (type.name === "bytes") return "Vec<u8>";
  }
  return typeName(type);
}

function encodeExpr(
  valueExpr: string,
  type: TypeRef,
  named: Map<string, Declaration>,
): string {
  if (type.kind === "void") return "Ok(Value::Null)";
  if (type.kind === "array") {
    return `encode_array(${valueExpr}, |item| ${
      encodeExpr("item", type.item, named)
    })`;
  }
  if (type.kind === "primitive") {
    if (type.name === "u53") return `encode_u53(*${valueExpr})`;
    if (type.name === "i53") return `encode_i53(*${valueExpr})`;
    if (type.name === "string") {
      return `Ok::<Value, CodecError>(Value::Text((*${valueExpr}).clone()))`;
    }
    if (type.name === "bool") {
      return `Ok::<Value, CodecError>(Value::Bool(*${valueExpr}))`;
    }
    if (type.name === "bytes") {
      return `Ok::<Value, CodecError>(Value::Bytes((*${valueExpr}).clone()))`;
    }
  }
  return `(${valueExpr}).encode_value()`;
}

function decodeExpr(
  valueExpr: string,
  type: TypeRef,
  named: Map<string, Declaration>,
): string {
  if (type.kind === "void") return "Ok(())";
  if (type.kind === "array") {
    return `decode_array(${valueExpr}, |item| ${
      decodeExpr("item", type.item, named)
    })`;
  }
  if (type.kind === "primitive") {
    if (type.name === "u53") return `expect_u64(${valueExpr})`;
    if (type.name === "i53") return `expect_i64(${valueExpr})`;
    if (type.name === "string") return `expect_text(${valueExpr})`;
    if (type.name === "bool") return `expect_bool(${valueExpr})`;
    if (type.name === "bytes") return `expect_bytes(${valueExpr})`;
  }
  return `${typeName(type)}::decode_value(${valueExpr})`;
}

function defaultForField(
  field: FieldDeclaration,
  named: Map<string, Declaration>,
): string {
  return field.optional ? "None" : defaultExpr(field.type, named);
}

function defaultExpr(type: TypeRef, named: Map<string, Declaration>): string {
  if (type.kind === "void") return "()";
  if (type.kind === "array") return "Vec::new()";
  if (type.kind === "primitive") {
    if (type.name === "string") return "String::new()";
    if (type.name === "bool") return "false";
    if (type.name === "bytes") return "Vec::new()";
    return "0";
  }
  const declaration = getNamed(type, named);
  if (declaration.kind === "enum") {
    return `${declaration.name}::${declaration.variants[0]!.name}`;
  }
  return `${declaration.name}::default()`;
}

function emitRuntimeHelpers(out: Writer) {
  out.line();
  out.line("const MAX_U53: u64 = 9_007_199_254_740_991;");
  out.line("const MIN_I53: i64 = -9_007_199_254_740_991;");
  out.line("const MAX_I53: i64 = 9_007_199_254_740_991;");
  out.line();
  out.line("fn encode_u53(value: u64) -> Result<Value, CodecError> {");
  out.indent(() => {
    out.line("if value > MAX_U53 {");
    out.indent(() =>
      out.line('return Err(CodecError::IntegerOutOfRange("u53"));')
    );
    out.line("}");
    out.line("Ok(Value::U64(value))");
  });
  out.line("}");
  out.line();
  out.line("fn encode_i53(value: i64) -> Result<Value, CodecError> {");
  out.indent(() => {
    out.line("if !(MIN_I53..=MAX_I53).contains(&value) {");
    out.indent(() =>
      out.line('return Err(CodecError::IntegerOutOfRange("i53"));')
    );
    out.line("}");
    out.line("Ok(Value::I64(value))");
  });
  out.line("}");
  out.line();
  out.line(
    "fn encode_array<T, F>(items: &[T], mut encode: F) -> Result<Value, CodecError>",
  );
  out.line("where");
  out.indent(() => {
    out.line("F: FnMut(&T) -> Result<Value, CodecError>,");
  });
  out.line("{");
  out.indent(() => {
    out.line("let mut values = Vec::with_capacity(items.len());");
    out.line("for item in items {");
    out.indent(() => out.line("values.push(encode(item)?);"));
    out.line("}");
    out.line("Ok(Value::Array(values))");
  });
  out.line("}");
  out.line();
  out.line(
    "fn decode_array<T, F>(value: &Value, mut decode: F) -> Result<Vec<T>, CodecError>",
  );
  out.line("where");
  out.indent(() => {
    out.line("F: FnMut(&Value) -> Result<T, CodecError>,");
  });
  out.line("{");
  out.indent(() => {
    out.line(
      "let Value::Array(items) = value else { return Err(CodecError::ExpectedArray); };",
    );
    out.line("let mut values = Vec::with_capacity(items.len());");
    out.line("for item in items {");
    out.indent(() => out.line("values.push(decode(item)?);"));
    out.line("}");
    out.line("Ok(values)");
  });
  out.line("}");
  out.line();
  out.line(
    "fn optional_field<T, F>(value: Option<&Value>, decode: F) -> Result<Option<T>, CodecError>",
  );
  out.line("where");
  out.indent(() => out.line("F: FnOnce(&Value) -> Result<T, CodecError>,"));
  out.line("{");
  out.indent(() => {
    out.line("match value {");
    out.indent(() => {
      out.line("Some(value) => decode(value).map(Some),");
      out.line("None => Ok(None),");
    });
    out.line("}");
  });
  out.line("}");
  out.line();
  out.line(
    "fn field_or_default<T, F, D>(value: Option<&Value>, decode: F, default: D) -> Result<T, CodecError>",
  );
  out.line("where");
  out.indent(() => {
    out.line("F: FnOnce(&Value) -> Result<T, CodecError>,");
    out.line("D: FnOnce() -> T,");
  });
  out.line("{");
  out.indent(() => {
    out.line("match value {");
    out.indent(() => {
      out.line("Some(value) => decode(value),");
      out.line("None => Ok(default()),");
    });
    out.line("}");
  });
  out.line("}");
  out.line();
  out.line(
    "fn expect_map(value: &Value) -> Result<&BTreeMap<u64, Value>, CodecError> {",
  );
  out.indent(() => {
    out.line(
      "let Value::Map(fields) = value else { return Err(CodecError::ExpectedMap); };",
    );
    out.line("Ok(fields)");
  });
  out.line("}");
  out.line();
  out.line(
    "fn expect_union(value: &Value) -> Result<(u64, &BTreeMap<u64, Value>), CodecError> {",
  );
  out.indent(() => {
    out.line(
      "let Value::Array(items) = value else { return Err(CodecError::ExpectedArray); };",
    );
    out.line("if items.len() != 2 { return Err(CodecError::ExpectedArray); }");
    out.line("let variant_id = expect_u64(&items[0])?;");
    out.line("let fields = expect_map(&items[1])?;");
    out.line("Ok((variant_id, fields))");
  });
  out.line("}");
  out.line();
  out.line("fn expect_u64(value: &Value) -> Result<u64, CodecError> {");
  out.indent(() => {
    out.line("match value {");
    out.indent(() => {
      out.line("Value::U64(value) => Ok(*value),");
      out.line("Value::I64(value) if *value >= 0 => Ok(*value as u64),");
      out.line("_ => Err(CodecError::ExpectedInteger),");
    });
    out.line("}");
  });
  out.line("}");
  out.line();
  out.line("fn expect_i64(value: &Value) -> Result<i64, CodecError> {");
  out.indent(() => {
    out.line("match value {");
    out.indent(() => {
      out.line("Value::I64(value) => Ok(*value),");
      out.line(
        "Value::U64(value) => i64::try_from(*value).map_err(|_| CodecError::ExpectedInteger),",
      );
      out.line("_ => Err(CodecError::ExpectedInteger),");
    });
    out.line("}");
  });
  out.line("}");
  out.line();
  out.line("fn expect_text(value: &Value) -> Result<String, CodecError> {");
  out.indent(() => {
    out.line(
      "let Value::Text(value) = value else { return Err(CodecError::ExpectedText); };",
    );
    out.line("Ok(value.clone())");
  });
  out.line("}");
  out.line();
  out.line("fn expect_bool(value: &Value) -> Result<bool, CodecError> {");
  out.indent(() => {
    out.line(
      "let Value::Bool(value) = value else { return Err(CodecError::ExpectedBool); };",
    );
    out.line("Ok(*value)");
  });
  out.line("}");
  out.line();
  out.line("fn expect_bytes(value: &Value) -> Result<Vec<u8>, CodecError> {");
  out.indent(() => {
    out.line(
      "let Value::Bytes(value) = value else { return Err(CodecError::ExpectedBytes); };",
    );
    out.line("Ok(value.clone())");
  });
  out.line("}");
}

function declarationMap(schema: Schema): Map<string, Declaration> {
  const map = new Map<string, Declaration>();
  for (const declaration of schema.declarations) {
    if (declaration.kind !== "proc") map.set(declaration.typePath, declaration);
  }
  return map;
}

function getNamed(
  type: TypeRef,
  named: Map<string, Declaration>,
): Exclude<Declaration, ProcDeclaration> {
  if (type.kind !== "named") throw new Error("expected named type");
  const declaration = named.get(type.path);
  if (!declaration || declaration.kind === "proc") {
    throw new Error(`unknown type ${type.path}`);
  }
  return declaration;
}

function typeName(type: TypeRef): string {
  if (type.kind !== "named") throw new Error("expected named type");
  return type.path.split(".").at(-1)!;
}

function fieldName(name: string): string {
  return rawIdent(snakeCase(name));
}

function snakeCase(name: string): string {
  return name
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1_$2")
    .replace(/[-\s]+/g, "_")
    .toLowerCase();
}

function rawIdent(name: string): string {
  return rustKeywords.has(name) ? `r#${name}` : name;
}

function requiredId(
  item: { attributes: Record<string, string>; name: string },
  label: string,
): number {
  const id = Number(requiredAttribute(item, "id", label));
  if (!Number.isSafeInteger(id) || id < 0) {
    throw new Error(`${label} requires integer @ id`);
  }
  return id;
}

function requiredProcStream(
  item: { attributes: Record<string, string>; name: string },
  label: string,
): ProcStream {
  const stream = requiredAttribute(item, "stream", label);
  if (
    stream !== "unary" && stream !== "server" && stream !== "client" &&
    stream !== "bidi"
  ) {
    throw new Error(`${label} requires @ stream to be unary, server, client, or bidi`);
  }
  return stream;
}

function requiredAttribute(
  item: { attributes: Record<string, string>; name: string },
  name: string,
  label: string,
): string {
  const value = item.attributes[name];
  if (value === undefined || value === "") {
    throw new Error(`${label} requires @ ${name}`);
  }
  return value;
}

function normalizePath(path: string): string {
  return path.replaceAll("\\", "/");
}

function toFileUrl(path: string): string {
  return new URL(normalizePath(path), `file:///${normalizePath(Deno.cwd())}/`)
    .href;
}

function dirname(path: string): string {
  const normalized = normalizePath(path);
  const index = normalized.lastIndexOf("/");
  return index < 0 ? "." : normalized.slice(0, index);
}

class Writer {
  #indent = "";
  #lines: string[] = [];

  line(text = "") {
    this.#lines.push(text.length === 0 ? "" : `${this.#indent}${text}`);
  }

  indent(write: () => void) {
    const previous = this.#indent;
    this.#indent += "    ";
    write();
    this.#indent = previous;
  }

  toString(): string {
    return `${this.#lines.join("\n")}\n`;
  }
}

if (import.meta.main) {
  const options = parseCli(Deno.args);
  const schema = await loadSchema(options.schemaRoots);
  const code = emitRust(schema, options);
  if (options.out) {
    await Deno.mkdir(dirname(options.out), { recursive: true });
    await Deno.writeTextFile(options.out, code);
  } else {
    console.log(code);
  }
}
