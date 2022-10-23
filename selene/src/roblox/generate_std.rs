use chrono::Local;
use color_eyre::eyre::Context;
use std::{collections::BTreeMap, io::Write};

use super::api::*;
use selene_lib::standard_library::*;

const API_DUMP: &str =
    "https://raw.githubusercontent.com/CloneTrooper1019/Roblox-Client-Tracker/roblox/API-Dump.json";

pub struct RobloxGenerator {
    pub std: StandardLibrary,
}

impl RobloxGenerator {
    pub fn generate() -> color_eyre::Result<(Vec<u8>, StandardLibrary)> {
        RobloxGenerator {
            std: StandardLibrary::roblox_base(),
        }
        .start_generation()
    }

    fn start_generation(mut self) -> color_eyre::Result<(Vec<u8>, StandardLibrary)> {
        let api: ApiDump = ureq::get(API_DUMP)
            .call()
            .context("error when getting API dump")?
            .into_json()
            .context("error when parsing API dump")?;

        self.write_class(&api, "game", "DataModel");
        self.write_class(&api, "plugin", "Plugin");
        self.write_class(&api, "script", "Script");
        self.write_class(&api, "workspace", "Workspace");

        self.write_enums(&api);
        self.write_instance_new(&api);
        self.write_get_service(&api);
        self.write_roblox_classes(&api);

        let mut bytes = Vec::new();

        let time = Local::now();
        self.std.last_updated = Some(time.timestamp());

        self.std.last_selene_version = Some(env!("CARGO_PKG_VERSION").to_owned());

        writeln!(
            bytes,
            "# This file was @generated by generate-roblox-std at {time}",
        )?;

        write!(bytes, "{}", serde_yaml::to_string(&self.std)?)?;

        self.std
            .extend(StandardLibrary::from_name(self.std.base.as_ref().unwrap()).unwrap());

        Ok((bytes, self.std))
    }

    fn write_class(&mut self, api: &ApiDump, global_name: &str, class_name: &str) {
        self.write_class_struct(api, class_name);
        self.std.globals.insert(
            global_name.to_owned(),
            Field::from_field_kind(FieldKind::Struct(class_name.to_owned())),
        );
    }

    fn write_class_struct(&mut self, api: &ApiDump, class_name: &str) {
        let structs = &mut self.std.structs;
        if structs.contains_key(class_name) {
            return;
        }

        structs.insert(class_name.to_owned(), BTreeMap::new());

        let mut table = BTreeMap::new();
        table.insert(
            "*".to_owned(),
            Field::from_field_kind(FieldKind::Struct("Instance".to_owned())),
        );

        self.write_class_members(api, &mut table, class_name);

        self.std.structs.insert(class_name.to_owned(), table);
    }

    fn write_class_members(
        &mut self,
        api: &ApiDump,
        table: &mut BTreeMap<String, Field>,
        class_name: &str,
    ) {
        let class = api.classes.iter().find(|c| c.name == class_name).unwrap();

        for member in &class.members {
            let (name, tags, field) = match &member {
                ApiMember::Callback { name, tags } => (
                    name,
                    tags,
                    Some(Field::from_field_kind(FieldKind::Property(
                        PropertyWritability::OverrideFields,
                    ))),
                ),

                ApiMember::Event { name, tags } => (
                    name,
                    tags,
                    Some(Field::from_field_kind(FieldKind::Struct(
                        "Event".to_owned(),
                    ))),
                ),

                ApiMember::Function {
                    name,
                    tags,
                    parameters,
                } => (
                    name,
                    tags,
                    Some(Field::from_field_kind(FieldKind::Function(
                        FunctionBehavior {
                            arguments: parameters
                                .iter()
                                .map(|_| Argument {
                                    argument_type: ArgumentType::Any,
                                    required: Required::NotRequired,
                                    observes: Observes::ReadWrite,
                                })
                                .collect(),
                            method: true,
                            must_use: false,
                        },
                    ))),
                ),

                ApiMember::Property {
                    name,
                    tags,
                    security,
                    value_type,
                } => (name, tags, {
                    if *security == ApiPropertySecurity::default() {
                        let empty = Vec::new();
                        let tags: &Vec<String> = match tags {
                            Some(tags) => tags,
                            None => &empty,
                        };

                        let default_field = Some(Field::from_field_kind(FieldKind::Property(
                            if tags.contains(&"ReadOnly".to_string()) {
                                PropertyWritability::ReadOnly
                            } else {
                                PropertyWritability::OverrideFields
                            },
                        )));

                        match &value_type {
                            ApiValueType::Class { name } => {
                                self.write_class_struct(api, name);
                                Some(Field::from_field_kind(FieldKind::Struct(name.to_owned())))
                            }

                            ApiValueType::DataType { value } => {
                                // See comment on `has_custom_methods` for why we're taking
                                // such a lax approach here.
                                if value.has_custom_methods() {
                                    Some(Field::from_field_kind(FieldKind::Any))
                                } else {
                                    default_field
                                }
                            }

                            _ => default_field,
                        }
                    } else {
                        None
                    }
                }),

                ApiMember::Unknown => {
                    // I want CI to fail when we see an unknown property, but fall back for users
                    if cfg!(test) {
                        panic!("unknown property found in Roblox API dump for {class_name}");
                    } else {
                        continue;
                    }
                }
            };

            let empty = Vec::new();
            let tags: &Vec<String> = match tags {
                Some(tags) => tags,
                None => &empty,
            };

            if let Some(mut field) = field {
                if tags.contains(&"Deprecated".to_owned()) {
                    field.deprecated = Some(Deprecated {
                        message: "this property is deprecated.".to_owned(),
                        replace: Vec::new(),
                    });
                }

                table.insert(name.to_owned(), field);
            }
        }

        if class.superclass != "<<<ROOT>>>" {
            self.write_class_members(api, table, &class.superclass);
        }
    }

    fn write_enums(&mut self, api: &ApiDump) {
        for enuhm in &api.enums {
            self.std.globals.insert(
                format!("Enum.{}.GetEnumItems", enuhm.name),
                Field::from_field_kind(FieldKind::Function(FunctionBehavior {
                    arguments: vec![],
                    method: true,
                    must_use: true,
                })),
            );

            for item in &enuhm.items {
                self.std.globals.insert(
                    format!("Enum.{}.{}", enuhm.name, item.name),
                    Field::from_field_kind(FieldKind::Struct("EnumItem".to_owned())),
                );
            }
        }
    }

    fn write_instance_new(&mut self, api: &ApiDump) {
        let instance_names = api
            .classes
            .iter()
            .filter_map(|class| {
                if !class.tags.contains(&"NotCreatable".to_owned()) {
                    Some(class.name.to_owned())
                } else {
                    None
                }
            })
            .collect();

        self.std.globals.insert(
            "Instance.new".to_owned(),
            Field::from_field_kind(FieldKind::Function(FunctionBehavior {
                arguments: vec![Argument {
                    argument_type: ArgumentType::Constant(instance_names),
                    required: Required::Required(None),
                    observes: Observes::ReadWrite,
                }],
                method: false,

                // Only true because we don't allow the second parameter
                must_use: true,
            })),
        );
    }

    fn write_get_service(&mut self, api: &ApiDump) {
        let service_names = api
            .classes
            .iter()
            .filter_map(|class| {
                if class.tags.contains(&"Service".to_owned()) {
                    Some(class.name.to_owned())
                } else {
                    None
                }
            })
            .collect();

        let data_model = self.std.structs.get_mut("DataModel").unwrap();

        *data_model.get_mut("GetService").unwrap() =
            Field::from_field_kind(FieldKind::Function(FunctionBehavior {
                arguments: vec![Argument {
                    argument_type: ArgumentType::Constant(service_names),
                    required: Required::Required(None),
                    observes: Observes::ReadWrite,
                }],
                method: true,
                must_use: true,
            }));
    }

    fn write_roblox_classes(&mut self, api: &ApiDump) {
        for class in &api.classes {
            let mut events = Vec::new();
            let mut properties = Vec::new();

            for member in &class.members {
                match member {
                    ApiMember::Event { name, .. } => events.push(name.to_owned()),
                    ApiMember::Property { name, .. } => properties.push(name.to_owned()),
                    _ => {}
                }
            }

            self.std.roblox_classes.insert(
                class.name.clone(),
                RobloxClass {
                    superclass: class.superclass.clone(),
                    events,
                    properties,
                },
            );
        }
    }
}
