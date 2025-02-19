use std::{error::Error, fs, path::Path};

use actix_web::{web, App, HttpServer, Responder};
use fake::{Fake, Faker};
use oxc::{
    allocator::Allocator,
    ast::ast::{Declaration, TSSignature},
    parser::{ParseOptions, Parser},
    span::{GetSpan, SourceType},
};
use serde_json::{json, Value};

fn parse_typescript_file(path: &Path, source_text: &str, allocator: &Allocator) -> Vec<Entity> {
    let source_type = SourceType::from_path(path).unwrap();
    let ret = Parser::new(allocator, source_text, source_type)
        .with_options(ParseOptions::default())
        .parse();

    let mut entities = Vec::new();

    for comment in &ret.program.comments {
        let comment_text = comment.content_span().source_text(source_text);
        let comment_parts: Vec<&str> = comment_text.split(" ").filter(|&x| !x.is_empty()).collect();

        match comment_parts.first() {
            Some(decl) if decl.contains("route") => (),
            _ => continue,
        };

        let route = match comment_parts.get(1) {
            Some(r) => r,
            None => continue,
        };

        if let Some(statement) = ret
            .program
            .body
            .iter()
            .find(|&x| x.span().start == comment.attached_to)
        {
            if let Declaration::TSInterfaceDeclaration(interface) = statement.to_declaration() {
                let mut entity = Entity {
                    route: String::from(*route),
                    props: Vec::new(),
                };

                for prop in interface.body.body.iter() {
                    if let TSSignature::TSPropertySignature(prop_sig) = prop {
                        if let (Some(name), Some(type_annot)) =
                            (prop_sig.key.name(), prop_sig.type_annotation.as_ref())
                        {
                            match type_annot.type_annotation {
                                oxc::ast::ast::TSType::TSBooleanKeyword(_) => {
                                    entity.props.push(Prop {
                                        id: name.to_string(),
                                        ty: TProp::Boolean,
                                    });
                                }
                                oxc::ast::ast::TSType::TSNumberKeyword(_) => {
                                    entity.props.push(Prop {
                                        id: name.to_string(),
                                        ty: TProp::Number,
                                    });
                                }
                                oxc::ast::ast::TSType::TSStringKeyword(_) => {
                                    entity.props.push(Prop {
                                        id: name.to_string(),
                                        ty: TProp::String,
                                    });
                                }
                                _ => continue,
                            }
                        }
                    }
                }
                entities.push(entity);
            }
        }
    }
    entities
}

fn scan_dir(dir: &Path, allocator: &Allocator) -> Result<Vec<Entity>, Box<dyn Error>> {
    let mut entities = Vec::new();
    let mut dirs_to_visit = vec![dir.to_path_buf()];

    while let Some(current_dir) = dirs_to_visit.pop() {
        for entry in fs::read_dir(&current_dir)? {
            let path = entry?.path();
            if path.is_dir() {
                dirs_to_visit.push(path);
            } else if let Some(ext) = path.extension() {
                if ext == "ts" || ext == "tsx" {
                    let source_text = fs::read_to_string(&path)?;
                    entities.extend(parse_typescript_file(&path, &source_text, allocator));
                }
            }
        }
    }
    Ok(entities)
}

#[derive(Debug, Clone)]
struct Entity {
    route: String,
    props: Vec<Prop>,
}

#[derive(Debug, Clone)]
struct Prop {
    id: String,
    ty: TProp,
}

#[derive(Debug, Clone)]
enum TProp {
    Boolean,
    Number,
    String,
}

async fn generate_fake_data(entity: web::Data<Entity>) -> impl Responder {
    let mut data = json!({});

    for prop in &entity.props {
        let value = match prop.ty {
            TProp::Boolean => Value::Bool(Faker.fake()),
            TProp::Number => Value::Number(serde_json::Number::from(
                fake::faker::number::en::NumberWithFormat("###")
                    .fake::<String>()
                    .parse::<i64>()
                    .unwrap(),
            )),
            TProp::String => Value::String(fake::faker::lorem::en::Word().fake()),
        };
        data[&prop.id] = value;
    }

    web::Json(data)
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let allocator = Allocator::default();
    let entities = scan_dir(&std::env::current_dir()?, &allocator)?;

    let app = HttpServer::new(move || {
        let mut app = App::new();
        for entity in entities.clone() {
            println!("{:?}", entity);
            app = app.service(
                web::resource(&entity.route)
                    .app_data(web::Data::new(entity.clone()))
                    .route(web::get().to(generate_fake_data)),
            );
        }
        app
    });

    app.bind("127.0.0.1:3000")?.run().await?;

    Ok(())
}
