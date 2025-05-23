#![allow(unused)] // todo remove this
use std::{collections::VecDeque, error::Error, sync::Arc};

use domain::{Config, ItemScope, Property, ValueType};
use log::debug;
use scraper::{ElementRef, Html, Selector, selectable::Selectable, selector::Parser};
use url::{ParseError, Url};

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
    )?;
    Ok(items)
}

// 5.2.4 Values
fn property_value<'a>(config: Config<'a>, element_ref: &ElementRef<'a>) -> ValueType {
    fn serialize_url<'a>(config: Config<'a>, url_elt: Option<&'a str>) -> ValueType {
        if let Some(url_elt) = url_elt {
            match url::Url::parse(url_elt) {
                Ok(url) => ValueType::Url(url.to_string()),
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
            .map(|s| ValueType::String(s.into()))
            .unwrap_or(ValueType::Empty),
        "meter" => element_ref
            .attr("value")
            .map(|s| ValueType::Meter(s.into())) // todo it's a numeric type
            .unwrap_or(ValueType::Empty),
        "time" => element_ref
            .attr("datetime")
            .map(|s| ValueType::Time(s.into())) // todo it's a datetime type
            .unwrap_or(ValueType::Empty),
        _ => ValueType::String(
            element_ref
                .text()
                .filter(|t| !t.trim().is_empty())
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .join(""),
        ),
    }
}

fn traverse<'a>(
    config: Config<'a>,
    document: &'a Html,
    element_ref: &ElementRef<'a>,
    parent: &mut Option<&mut VecDeque<Property>>,
    items: &mut VecDeque<ItemScope>,
) -> Result<(), Box<dyn Error>> {
    let mut id = element_ref.attr("id");
    let mut itemscope = element_ref.attr("itemscope");
    let itemrefs = element_ref.attr("itemref").map(|r| {
        r.split(" ")
            .filter(|r| !r.trim().is_empty())
            .map(|r| format!("#{r}"))
            .collect::<Vec<_>>()
    });
    let mut itemprops = element_ref.attr("itemprop").map(|r| {
        r.split(" ")
            .filter(|r| !r.trim().is_empty())
            .collect::<Vec<_>>()
    });
    if let Some(itemscope) = itemscope.take() {
        let mut itemscope = ItemScope {
            ..Default::default()
        };
        if let Some(itemrefs) = itemrefs {
            for itemref in itemrefs {
                let selector = Selector::parse(itemref.as_str()).map_err(|e| e.to_string())?;
                let elts = document.select(&selector);
                for elt in elts {
                    traverse(
                        config,
                        document,
                        &elt,
                        &mut Some(&mut itemscope.items),
                        items,
                    )?;
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
            )?;
        }
        if let Some(itemprops) = itemprops.take() {
            let itemscope = Arc::new(itemscope);
            for itemprop in itemprops {
                if let Some(parent) = parent.as_deref_mut() {
                    parent.push_back(Property {
                        name: domain::Name::String(itemprop.to_string()),
                        value: domain::ValueType::ScopeRef(itemscope.clone()),
                    });
                }
            }
        } else {
            items.push_back(itemscope);
        }
    } else if let Some(itemprops) = itemprops.take() {
        for itemprop in itemprops {
            if let Some(parent) = parent.as_deref_mut() {
                parent.push_back(Property {
                    name: domain::Name::String(itemprop.to_string()),
                    value: property_value(config, element_ref),
                });
            }
        }
    } else {
        for child in element_ref.child_elements() {
            // check what's next
            traverse(config, document, &child, parent, items)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use std::{collections::VecDeque, sync::Arc};

    use crate::{
        domain::{ItemScope, Name, Property, ValueType},
        parse_html,
    };

    #[test]
    fn test_example1() {
        let expected = vec![
            ItemScope {
                id: None,
                items: vec![Property {
                    name: Name::String("name".to_string()),
                    value: ValueType::String("Elizabeth".to_string()),
                }]
                .into(),
            },
            ItemScope {
                id: None,
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
                id: None,
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
                id: None,
                items: VecDeque::from([Property {
                    name: Name::String("image".to_string()),
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
                id: None,
                items: VecDeque::from([Property {
                    name: Name::String("product-id".to_string()),
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
                id: None,
                items: VecDeque::from([
                    Property {
                        name: Name::String("name".to_string()),
                        value: ValueType::String("Panasonic White 60L Refrigerator".into())
                    },
                    Property {
                        name: Name::String("aggregateRating".to_string()),
                        value: ValueType::ScopeRef(Arc::new(ItemScope {
                            id: None,
                            items: vec![
                                Property {
                                    name: Name::String("ratingValue".to_string()),
                                    value: ValueType::Meter("3.5".into())
                                },
                                Property {
                                    name: Name::String("reviewCount".to_string()),
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
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                id: None,
                items: VecDeque::from([Property {
                    name: Name::String("birthday".to_string()),
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
                id: None,
                items: VecDeque::from([
                    Property {
                        name: Name::String("name".to_string()),
                        value: ValueType::String("Amanda".into())
                    },
                    Property {
                        name: Name::String("band".to_string()),
                        value: ValueType::ScopeRef(Arc::new(ItemScope {
                            id: None,
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
        let res = parse_html("http://bittich.be/", html).unwrap();
        assert_eq!(
            res,
            VecDeque::from([ItemScope {
                id: None,
                items: VecDeque::from([
                    Property {
                        name: Name::String("name".to_string()),
                        value: ValueType::String("Amanda".into())
                    },
                    Property {
                        name: Name::String("band".to_string()),
                        value: ValueType::ScopeRef(Arc::new(ItemScope {
                            id: None,
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
                id: None,
                items: VecDeque::from([
                    Property {
                        name: Name::String("flavor".to_string()),
                        value: ValueType::String("Lemon sorbet".into())
                    },
                    Property {
                        name: Name::String("flavor".to_string()),
                        value: ValueType::String("Apricot sorbet".into())
                    },
                ])
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
                id: None,
                items: VecDeque::from([
                    Property {
                        name: Name::String("favorite-color".to_string()),
                        value: ValueType::String("orange".into())
                    },
                    Property {
                        name: Name::String("favorite-fruit".to_string()),
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
                id: None,
                items: VecDeque::from([Property {
                    name: Name::String("name".to_string()),
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
}
