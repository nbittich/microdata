use std::{
    collections::{BTreeSet, VecDeque},
    error::Error,
    sync::Arc,
};

use domain::{Config, ItemScope, Name, Property, ValueType};
use log::debug;
use scraper::{ElementRef, Html, Selector};
use url::Url;

pub mod domain;

pub fn parse_html<'a>(
    base_url: &'a str,
    html: &'a str,
) -> Result<VecDeque<ItemScope>, Box<dyn Error>> {
    let base_url = if base_url.ends_with("/") {
        &base_url[0..base_url.len() - 1]
    } else {
        base_url
    };
    let mut items = VecDeque::new();
    let document = scraper::Html::parse_document(html);

    traverse(
        Config { base_url },
        &document,
        &document.root_element(),
        &mut None,
        &mut items,
        &mut BTreeSet::new(),
    )?;
    Ok(items)
}

// 5.2.4 Values
fn serialize_url<'a>(config: Config<'a>, url_elt: Option<&'a str>) -> ValueType {
    if let Some(url_elt) = url_elt {
        match url::Url::parse(url_elt.trim()) {
            Ok(url) => ValueType::Url(url.to_string()),
            Err(_) if config.base_url.is_empty() => ValueType::String(url_elt.trim().to_string()), // assume relative
            Err(e) => {
                debug!("could not parse url {e}");
                let url_elt = if url_elt.starts_with("/") {
                    &url_elt[0..url_elt.len() - 1]
                } else {
                    url_elt
                };
                let absolute_url = format!("{}/{url_elt}", config.base_url);
                Url::parse(&absolute_url)
                    .inspect_err(|e| debug!("still cannot parse url even with a base! {e}"))
                    .ok()
                    .map(|u| ValueType::Url(u.to_string()))
                    .unwrap_or(ValueType::Empty)
            }
        }
    } else {
        ValueType::Empty
    }
}

fn property_value<'a>(config: Config<'a>, element_ref: &ElementRef<'a>) -> ValueType {
    match element_ref.value().name() {
        "meta" => element_ref
            .attr("content")
            .map(|s| ValueType::String(s.into()))
            .unwrap_or(ValueType::Empty),
        "audio" | "embed" | "iframe" | "img" | "source" | "track" | "video" => {
            serialize_url(config, element_ref.attr("src"))
        }
        "a" | "area" | "link" => serialize_url(config, element_ref.attr("href")),
        "object" => serialize_url(config, element_ref.attr("data")),
        "data" => element_ref
            .attr("value")
            .map(|s| ValueType::String(s.trim().into()))
            .unwrap_or(ValueType::Empty),
        "meter" => element_ref
            .attr("value")
            .map(|s| ValueType::Meter(s.trim().into())) // todo it's a numeric type
            .unwrap_or(ValueType::Empty),
        "time" => element_ref
            .attr("datetime")
            .map(|s| ValueType::Time(s.trim().into())) // todo it's a datetime type
            .unwrap_or(ValueType::Empty),
        _ => ValueType::String(
            element_ref
                .text()
                .filter(|t| !t.trim().is_empty())
                .map(|t| t.trim().to_string())
                .collect::<Vec<_>>()
                .join(""),
        ),
    }
}

fn serialize_name(config: Config, itemprop: String) -> Result<Name, Box<dyn Error>> {
    match serialize_url(config, Some(itemprop.as_str())) {
        ValueType::Url(url) => Ok(domain::Name::Url(url)),
        _ if !itemprop
            .chars()
            .any(|b| ['\u{003A}', '\u{002E}'].contains(&b)) =>
        {
            Ok(domain::Name::String(itemprop.to_string()))
        }
        _ => Err(format!("itemprop {itemprop} is not a valid property").into()),
    }
}
fn traverse<'a>(
    config: Config<'a>,
    document: &'a Html,
    element_ref: &ElementRef<'a>,
    parent: &mut Option<&mut VecDeque<Property>>,
    items: &mut VecDeque<ItemScope>,
    in_ref: &mut BTreeSet<Option<&'a str>>,
) -> Result<(), Box<dyn Error>> {
    let itemscope = element_ref.attr("itemscope");
    let itemid = element_ref.attr("itemid").map(|r| r.trim().to_string());
    let itemtype = element_ref
        .attr("itemtype")
        .map(|r| {
            r.split(" ")
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .filter(|r| Url::parse(r).is_ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let itemrefs = element_ref.attr("itemref").map(|r| {
        r.split(" ")
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
            .collect::<Vec<_>>()
    });
    let mut itemprops = element_ref.attr("itemprop").map(|r| {
        r.split(" ")
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
            .collect::<Vec<_>>()
    });
    if itemscope.is_some() {
        let mut itemscope = ItemScope {
            itemtype,
            itemid,
            ..Default::default()
        };
        if let Some(itemrefs) = itemrefs {
            if itemrefs.iter().any(|s| in_ref.contains(&Some(s))) {
                return Err(format!("cycle detected! {in_ref:?}").into());
            }
            for itemref in itemrefs {
                let selector =
                    Selector::parse(&format!("#{itemref}")).map_err(|e| e.to_string())?;
                let elts = document.select(&selector);
                for elt in elts {
                    if elt.attr("id").is_some() {
                        in_ref.insert(elt.attr("id"));
                    }
                    traverse(
                        config,
                        document,
                        &elt,
                        &mut Some(&mut itemscope.items),
                        items,
                        in_ref,
                    )?;
                    in_ref.remove(&elt.attr("id"));
                }
            }
        }
        for child in element_ref.child_elements() {
            traverse(
                config,
                document,
                &child,
                &mut Some(&mut itemscope.items),
                items,
                in_ref,
            )?;
        }
        if let Some(itemprops) = itemprops.take() {
            let itemscope = Arc::new(itemscope);
            for itemprop in itemprops {
                if let Some(parent) = parent.as_deref_mut() {
                    let name = serialize_name(config, itemprop)?;
                    let value = domain::ValueType::ScopeRef(itemscope.clone());
                    // let prop = Property { name, value };
                    if let Some(prop_with_same_name) = parent.iter_mut().find(|p| p.name == name) {
                        match &mut prop_with_same_name.value {
                            ValueType::Array(value_types) => value_types.push_back(value),
                            _ => {
                                prop_with_same_name.value = ValueType::Array(VecDeque::from([
                                    prop_with_same_name.value.clone(),
                                    value,
                                ]));
                            }
                        }
                    } else {
                        parent.push_back(Property { name, value });
                    }
                }
            }
        } else {
            items.push_back(itemscope);
        }
    } else if let Some(itemprops) = itemprops.take() {
        for itemprop in itemprops {
            if let Some(parent) = parent.as_deref_mut() {
                let name = serialize_name(config, itemprop)?;
                let value = property_value(config, element_ref);
                // let prop = Property { name, value };
                if let Some(prop_with_same_name) = parent.iter_mut().find(|p| p.name == name) {
                    match &mut prop_with_same_name.value {
                        ValueType::Array(value_types) => value_types.push_back(value),
                        _ => {
                            prop_with_same_name.value = ValueType::Array(VecDeque::from([
                                prop_with_same_name.value.clone(),
                                value,
                            ]));
                        }
                    }
                } else {
                    parent.push_back(Property { name, value });
                }
            }
        }
    } else {
        for child in element_ref.child_elements() {
            // check what's next
            traverse(config, document, &child, parent, items, in_ref)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use std::{
        collections::{BTreeSet, VecDeque},
        sync::Arc,
    };

    use crate::{
        domain::{ItemScope, Name, Property, ValueType},
        parse_html,
    };

    #[test]
    fn test_example1() {
        let expected = vec![
            ItemScope {
                itemtype: vec![],
                itemid: None,
                items: vec![Property {
                    name: Name::String("name".to_string()),
                    value: ValueType::String("Elizabeth".to_string()),
                }]
                .into(),
            },
            ItemScope {
                itemid: None,
                itemtype: vec![],
                items: vec![Property {
                    name: Name::String("name".to_string()),
                    value: ValueType::String("Daniel".to_string()),
                }]
                .into(),
            },
        ]
        .into_iter()
        .collect::<VecDeque<_>>();
        let html = r#"
        <div itemscope>
            <p>My name is <span itemprop="name">Elizabeth</span>.</p>
        </div>
        <div itemscope>
            <p>My name is <span itemprop="name">Daniel</span>.</p>
        </div>
        "#;
        let res = parse_html("", html).unwrap();
        assert_eq!(res, expected,);
        let html = r#"
        <div itemscope>
            <p>My <em>name</em> is <span itemprop="name">E<strong>liz</strong>abeth</span>.</p>
        </div>
        <section>
            <div itemscope>
                 <aside>
                    <p>My name is <span itemprop="name"><a href="/?user=daniel">Daniel</a></span>.</p>
                 </aside>
            </div>
        </section>
        "#;
        let res = parse_html("", html).unwrap();
        assert_eq!(res, expected,);
    }
    #[test]
    fn test_example2() {
        let html = r#"
            <div itemscope>
                <p>My name is <span itemprop="name">Neil</span>.</p>
                <p>My band is called <span itemprop="band">Four Parts Water</span>.</p>
                <p>I am <span itemprop="nationality">British</span>.</p>
            </div>        
        "#;
        let res = parse_html("", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([
                    Property {
                        name: Name::String("name".to_string()),
                        value: ValueType::String("Neil".into())
                    },
                    Property {
                        name: Name::String("band".to_string()),
                        value: ValueType::String("Four Parts Water".into())
                    },
                    Property {
                        name: Name::String("nationality".to_string()),
                        value: ValueType::String("British".into())
                    },
                ])
            }])
        );
    }

    #[test]
    fn test_example3() {
        let html = r#"
            <div itemscope>
            <img itemprop="image" src="google-logo.png" alt="Google">
            </div>      
        "#;
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([Property {
                    name: Name::Url("http://bittich.be/image".to_string()),
                    value: ValueType::Url("http://bittich.be/google-logo.png".into())
                }])
            }])
        );
    }

    #[test]
    fn test_example4() {
        let html = r#"
           <h1 itemscope>
                <data itemprop="product-id" value="9678AOU879">The Instigator 2000</data>
           </h1>   
        "#;
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([Property {
                    name: Name::Url("http://bittich.be/product-id".to_string()),
                    value: ValueType::String("9678AOU879".into())
                }])
            }])
        );
    }

    #[test]
    fn test_example5() {
        let html = r#"
            <div itemscope itemtype="http://schema.org/Product">
                <span itemprop="name">Panasonic White 60L Refrigerator</span>
                <img src="panasonic-fridge-60l-white.jpg" alt="">
                <div itemprop="aggregateRating"
                    itemscope itemtype="http://schema.org/AggregateRating">
                    <meter itemprop="ratingValue" min=0 value=3.5 max=5>Rated 3.5/5</meter>
                    (based on <span itemprop="reviewCount">11</span> customer reviews)
                </div>
            </div> 
        "#;
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec!["http://schema.org/Product".into()],
                items: VecDeque::from([
                    Property {
                        name: Name::Url("http://bittich.be/name".to_string()),
                        value: ValueType::String("Panasonic White 60L Refrigerator".into())
                    },
                    Property {
                        name: Name::Url("http://bittich.be/aggregateRating".to_string()),
                        value: ValueType::ScopeRef(Arc::new(ItemScope {
                            itemtype: vec!["http://schema.org/AggregateRating".into()],
                            itemid: None,
                            items: vec![
                                Property {
                                    name: Name::Url("http://bittich.be/ratingValue".to_string()),
                                    value: ValueType::Meter("3.5".into())
                                },
                                Property {
                                    name: Name::Url("http://bittich.be/reviewCount".to_string()),
                                    value: ValueType::String("11".into())
                                },
                            ]
                            .into()
                        }))
                    },
                ])
            }])
        );
    }
    #[test]
    fn test_example6() {
        let html = r#"
            <div itemscope>
            I was born on <time itemprop="birthday" datetime="2009-05-10">May 10th 2009</time>.
            </div>  
        "#;
        let res = parse_html("http://bittich.be", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([Property {
                    name: Name::Url("http://bittich.be/birthday".to_string()),
                    value: ValueType::Time("2009-05-10".into())
                }])
            }])
        );
    }
    #[test]
    fn test_example7() {
        let html = r#"
            <div itemscope>
            <p>Name: <span itemprop="name">Amanda</span></p>
            <p>Band: <span itemprop="band" itemscope> <span itemprop="name">Jazz Band</span> (<span itemprop="size">12</span> players)</span></p>
            </div>
        "#;
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([
                    Property {
                        name: Name::Url("http://bittich.be/name".to_string()),
                        value: ValueType::String("Amanda".into())
                    },
                    Property {
                        name: Name::Url("http://bittich.be/band".to_string()),
                        value: ValueType::ScopeRef(Arc::new(ItemScope {
                            itemtype: vec![],
                            itemid: None,
                            items: vec![
                                Property {
                                    name: Name::Url("http://bittich.be/name".to_string()),
                                    value: ValueType::String("Jazz Band".into())
                                },
                                Property {
                                    name: Name::Url("http://bittich.be/size".to_string()),
                                    value: ValueType::String("12".into())
                                },
                            ]
                            .into()
                        }))
                    },
                ])
            }])
        );
    }

    #[test]
    fn test_example8() {
        let html = r#"
        <div itemscope id="amanda" itemref="a b"></div>
        <p id="a">Name: <span itemprop="name">Amanda</span></p>
        <div id="b" itemprop="band" itemscope itemref="c"></div>
        <div id="c">
        <p>Band: <span itemprop="name">Jazz Band</span></p>
        <p>Size: <span itemprop="size">12</span> players</p>
        </div>
        "#;
        let res = parse_html("", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([
                    Property {
                        name: Name::String("name".to_string()),
                        value: ValueType::String("Amanda".into())
                    },
                    Property {
                        name: Name::String("band".to_string()),
                        value: ValueType::ScopeRef(Arc::new(ItemScope {
                            itemtype: vec![],
                            itemid: None,
                            items: vec![
                                Property {
                                    name: Name::String("name".to_string()),
                                    value: ValueType::String("Jazz Band".into())
                                },
                                Property {
                                    name: Name::String("size".to_string()),
                                    value: ValueType::String("12".into())
                                },
                            ]
                            .into()
                        }))
                    },
                ])
            }])
        );
    }

    #[test]
    fn test_example9() {
        let html = r#"
            <div itemscope>
            <p>Flavors in my favorite ice cream:</p>
            <ul>
            <li itemprop="flavor">Lemon sorbet</li>
            <li itemprop="flavor">Apricot sorbet</li>
            </ul>
            </div>
        "#;
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([Property {
                    name: Name::Url("http://bittich.be/flavor".to_string()),
                    value: ValueType::Array(
                        [
                            ValueType::String("Lemon sorbet".into()),
                            ValueType::String("Apricot sorbet".into())
                        ]
                        .into()
                    )
                }])
            }])
        );
    }

    #[test]
    fn test_example10() {
        let html = r#"
            <div itemscope>
            <span itemprop="favorite-color favorite-fruit">orange</span>
            </div>
        "#;
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([
                    Property {
                        name: Name::Url("http://bittich.be/favorite-color".to_string()),
                        value: ValueType::String("orange".into())
                    },
                    Property {
                        name: Name::Url("http://bittich.be/favorite-fruit".to_string()),
                        value: ValueType::String("orange".into())
                    },
                ])
            }])
        );
    }

    #[test]
    fn test_example11() {
        let html = r#"
            <figure>
            <img src="castle.jpeg">
            <figcaption><span itemscope><span itemprop="name">The Castle</span></span> (1986)</figcaption>
            </figure>        "#;
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: None,
                itemtype: vec![],
                items: VecDeque::from([Property {
                    name: Name::Url("http://bittich.be/name".to_string()),
                    value: ValueType::String("The Castle".into())
                },])
            }])
        );
        let html = r#"
            <span itemscope><meta itemprop="name" content="The Castle"></span>
            <figure>
            <img src="castle.jpeg">
            <figcaption>The Castle (1986)</figcaption>
            </figure>      
            "#;
        let res2 = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(res, res2);
    }
    #[test]
    fn test_example12() {
        let html = r#"
        <div itemscope id="amanda" itemref="a b"></div>
        <p id="a">Name: <span itemprop="name">Amanda</span></p>
        <div id="b" itemprop="band" itemscope itemref="c"></div>
        <div id="c">
        <p>Band: <span itemprop="name">Jazz Band</span></p>
        <p>Band: <span itemprop="name" itemscope itemref="amanda">Jazz Band</span></p>
        <p>Size: <span itemprop="size">12</span> players</p>
        </div>
        "#;
        let res = parse_html("http://bittich.be/", html);
        assert_eq!(
            res.err().map(|e| e.to_string()),
            Some(format!(
                "cycle detected! {:?}",
                BTreeSet::from([Some("amanda"), Some("b"), Some("c")])
            ))
        );

        assert_eq!(
            parse_html(
                "http://bittich.be",
                r#"
        <div itemscope itemtype="http://schema.org/Person" id="person1" itemref="person1">
         <span itemprop="name">Alice</span>
        </div>
        "#
            )
            .err()
            .map(|s| s.to_string()),
            Some(format!(
                "cycle detected! {:?}",
                BTreeSet::from([Some("person1")])
            ))
        );

        assert_eq!(
            parse_html(
                "http://bittich.be",
                r#"
        <div itemscope itemtype="http://schema.org/Person" id="person1" itemref="person2">
        <span itemprop="name">Bob</span>
        </div>
        <div itemscope itemtype="http://schema.org/Person" id="person2" itemref="person1">
        <span itemprop="name">Carol</span>
        </div>
        "#
            )
            .err()
            .map(|s| s.to_string()),
            Some(format!(
                "cycle detected! {:?}",
                BTreeSet::from([Some("person1"), Some("person2")])
            ))
        );
        assert_eq!(
            parse_html(
                "http://bittich.be",
                r#"
                <div itemscope itemtype="http://schema.org/Person" id="a" itemref="b">
                <span itemprop="name">Dave</span>
                </div>

                <div itemscope itemtype="http://schema.org/Person" id="b" itemref="c">
                <span itemprop="name">Eve</span>
                </div>

                <div itemscope itemtype="http://schema.org/Person" id="c" itemref="a">
                <span itemprop="name">Frank</span>
                </div>        
                "#
            )
            .err()
            .map(|s| s.to_string()),
            Some(format!(
                "cycle detected! {:?}",
                BTreeSet::from([Some("a"), Some("b"), Some("c")])
            ))
        );
        assert_eq!(
            parse_html(
                "http://bittich.be",
                r#"
                <div itemscope itemtype="http://schema.org/Organization" id="org" itemref="team leader">
                <span itemprop="name">TechCorp</span>
                </div>

                <div itemscope itemtype="http://schema.org/Person" id="leader" itemref="org team">
                <span itemprop="name">Grace</span>
                </div>

                <div itemscope itemtype="http://schema.org/Person" id="team" itemref="leader">
                <span itemprop="name">Heidi</span>
                </div>     
                "#
            )
            .err()
            .map(|s| s.to_string()),
            Some(format!(
                "cycle detected! {:?}",
                BTreeSet::from([Some("leader"),Some("team")])
            ))
        );
        assert_eq!(
            parse_html(
                "http://bittich.be",
                r#"
                    <div itemscope itemtype="http://schema.org/Event" id="event" itemref="venue">
                    <span itemprop="name">Conference 2025</span>
                    </div>

                    <div itemscope itemtype="http://schema.org/Place" id="venue" itemref="organizer">
                    <span itemprop="name">City Hall</span>
                    </div>

                    <div itemscope itemtype="http://schema.org/Organization" id="organizer" itemref="event">
                    <span itemprop="name">TechGroup</span>
                    </div>  
                "#
            )
            .err()
            .map(|s| s.to_string()),
            Some(format!(
                "cycle detected! {:?}",
                BTreeSet::from([Some("event"),Some("organizer"), Some("venue")])
            ))
        );
    }

    #[test]
    fn test_example13() {
        let html = r#"
          <dl itemscope
            itemtype="https://vocab.example.net/book"
            itemid="urn:isbn:0-330-34032-8">
        <dt>Title
        <dd itemprop="title">The Reality Dysfunction
        <dt>Author
        <dd itemprop="author">Peter F. Hamilton
        <dt>Publication date
        <dd><time itemprop="pubdate" datetime="1996-01-26">26 January 1996</time>
        </dl>
        "#;
        let res = parse_html("http://bittich.be", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                itemid: Some("urn:isbn:0-330-34032-8".into()),
                itemtype: vec!["https://vocab.example.net/book".into()],
                items: VecDeque::from([
                    Property {
                        name: Name::Url("http://bittich.be/title".to_string()),
                        value: ValueType::String("The Reality Dysfunction".into())
                    },
                    Property {
                        name: Name::Url("http://bittich.be/author".to_string()),
                        value: ValueType::String("Peter F. Hamilton".into())
                    },
                    Property {
                        name: Name::Url("http://bittich.be/pubdate".to_string()),
                        value: ValueType::Time("1996-01-26".into())
                    },
                ])
            }])
        );
    }
    #[test]
    fn test_example14() {
        let html = r#"
        <div itemscope>
            <p itemprop="a">1</p>
            <p itemprop="a">2</p>
            <p itemprop=":b">test</p>
        </div>
        "#;
        let res = parse_html("", html);
        assert_eq!(
            res.err().map(|s| s.to_string()),
            Some("itemprop :b is not a valid property".to_string())
        );
    }

    #[test]
    fn test_example15() {
        let html = r#"
                <section id="jack" itemscope itemtype="http://microformats.org/profile/hcard">
                <h1 itemprop="fn">
                <span itemprop="n" itemscope>
                <span itemprop="given-name">Jack</span>
                <span itemprop="family-name">Bauer</span>
                </span>
                </h1>
                <img itemprop="photo" alt="" src="jack-bauer.jpg">
                <p itemprop="org" itemscope>
                <span itemprop="organization-name">Counter-Terrorist Unit</span>
                (<span itemprop="organization-unit">Los Angeles Division</span>)
                </p>
                <p>
                <span itemprop="adr" itemscope>
                <span itemprop="street-address">10201 W. Pico Blvd.</span><br>
                <span itemprop="locality">Los Angeles</span>,
                <span itemprop="region">CA</span>
                <span itemprop="postal-code">90064</span><br>
                <span itemprop="country-name">United States</span><br>
                </span>
                <span itemprop="geo">34.052339;-118.410623</span>
                </p>
                <h2>Assorted Contact Methods</h2>
                <ul>
                <li itemprop="tel" itemscope>
                <span itemprop="value">+1 (310) 597 3781</span> <span itemprop="type">work</span>
                <meta itemprop="type" content="voice">
                </li>
                <li><a itemprop="url" href="https://en.wikipedia.org/wiki/Jack_Bauer">I'm on Wikipedia</a>
                so you can leave a message on my user talk page.</li>
                <li><a itemprop="url" href="http://www.jackbauerfacts.com/">Jack Bauer Facts</a></li>
                <li itemprop="email"><a href="mailto:j.bauer@la.ctu.gov.invalid">j.bauer@la.ctu.gov.invalid</a></li>
                <li itemprop="tel" itemscope>
                <span itemprop="value">+1 (310) 555 3781</span> <span>
                <meta itemprop="type" content="cell">mobile phone</span>
                </li>
                </ul>
                <ins datetime="2008-07-20 21:00:00+01:00">
                <meta itemprop="rev" content="2008-07-20 21:00:00+01:00">
                <p itemprop="tel" itemscope><strong>Update!</strong>
                My new <span itemprop="type">home</span> phone number is
                <span itemprop="value">01632 960 123</span>.</p>
                </ins>
                </section>
                <address itemscope itemtype="http://microformats.org/profile/hcard">
                    <strong itemprop="fn"><span itemprop="n" itemscope><span itemprop="given-name">Alfred</span>
                    <span itemprop="family-name">Person</span></span></strong> <br>
                    <span itemprop="adr" itemscope>
                    <span itemprop="street-address">1600 Amphitheatre Parkway</span> <br>
                    <span itemprop="street-address">Building 43, Second Floor</span> <br>
                    <span itemprop="locality">Mountain View</span>,
                    <span itemprop="region">CA</span> <span itemprop="postal-code">94043</span>
                    </span>
                    </address>
                    <span itemscope itemtype="http://microformats.org/profile/hcard"
                    ><span itemprop=fn><span itemprop="n" itemscope><span itemprop="given-name"
                    >George</span> <span itemprop="family-name">Washington</span></span
                    ></span></span>
        "#;
        let res = parse_html("", html).unwrap();
        println!("{}", serde_json::to_string_pretty(&res).unwrap());
    }
}
