//! Count the real page tree; only materialize the requested page descriptors.

use std::collections::HashSet;

use super::{Dict, Document, Error, Object, Page, Rc, Result, malformed};

pub struct Selection {
    pub count: u32,
    pub pages: Vec<(u32, Result<Page>)>,
    first: u32,
    last: u32,
    maximum: usize,
}

impl Selection {
    fn add(&mut self, page: impl FnOnce() -> Result<Page>) -> Result<()> {
        self.count = self.count.checked_add(1).ok_or(Error::LimitExceeded("page count"))?;
        if (self.first..=self.last).contains(&self.count) && self.pages.len() < self.maximum {
            self.pages.push((self.count, page()));
        }
        Ok(())
    }
}

impl Document<'_> {
    pub fn pages(&self) -> Result<Vec<Page>> {
        self.select_pages(1, u32::MAX, usize::MAX)?
            .pages
            .into_iter()
            .map(|(_, page)| page)
            .collect()
    }

    pub fn select_pages(&self, first: u32, last: u32, maximum: usize) -> Result<Selection> {
        let mut selected = Selection { count: 0, pages: Vec::new(), first, last, maximum };
        if let Some(root) = self.trailer().get(b"Root") {
            let root = self.resolve(root)?;
            if let Some(tree) = root.as_dict().and_then(|root| root.get(b"Pages")) {
                self.walk(tree, &Inherited::default(), 0, &mut HashSet::new(), &mut selected)?;
            }
        }
        if selected.count == 0 {
            let mut numbers: Vec<u32> = self.xref.entries.keys().copied().collect();
            numbers.sort_unstable();
            for number in numbers {
                self.budget.check()?;
                let object = self.get(number)?;
                if let Some(dict) = object.as_dict()
                    && dict.has_name(b"Type", b"Page")
                {
                    selected.add(|| self.page(dict, &Inherited::default()))?;
                }
            }
        }
        if selected.count == 0 {
            return Err(malformed("the document has no pages"));
        }
        Ok(selected)
    }

    fn walk(
        &self,
        node: &Object,
        inherited: &Inherited,
        depth: usize,
        visited: &mut HashSet<u32>,
        selected: &mut Selection,
    ) -> Result<()> {
        self.budget.check()?;
        self.budget.nest(depth)?;
        if let Some((number, _)) = node.as_reference()
            && !visited.insert(number)
        {
            return Ok(());
        }
        let node = self.resolve(node)?;
        let Some(dict) = node.as_dict() else { return Ok(()) };
        if dict.has_name(b"Type", b"Page") {
            return selected.add(|| self.page(dict, inherited));
        }
        let inherited = self.inherit(dict, inherited)?;
        if let Some(Object::Array(kids)) = self.entry(dict, b"Kids")?.as_deref() {
            for kid in kids {
                self.walk(kid, &inherited, depth + 1, visited, selected)?;
            }
        }
        Ok(())
    }

    fn inherit(&self, dict: &Dict, inherited: &Inherited) -> Result<Inherited> {
        let resources = match dict.get(b"Resources") {
            Some(object) => Some(self.resolve(object)?.into_shared()),
            None => inherited.resources.clone(),
        };
        let media_box = match self.entry(dict, b"MediaBox")?.as_deref().and_then(Object::as_array) {
            Some(corners) if corners.len() == 4 => {
                let mut media_box = [0.0; 4];
                for (slot, corner) in media_box.iter_mut().zip(corners) {
                    *slot = self.resolve(corner)?.as_number().unwrap_or(0.0);
                }
                media_box
            }
            _ => inherited.media_box,
        };
        let rotate =
            self.entry(dict, b"Rotate")?.and_then(|it| it.as_integer()).unwrap_or(inherited.rotate);
        Ok(Inherited { resources, media_box, rotate })
    }

    fn page(&self, dict: &Dict, inherited: &Inherited) -> Result<Page> {
        let inherited = self.inherit(dict, inherited)?;
        let contents = match dict.get(b"Contents") {
            Some(reference @ Object::Ref(..)) => match &*self.resolve(reference)? {
                Object::Array(parts) => parts.clone(),
                _ => vec![reference.clone()],
            },
            Some(Object::Array(parts)) => parts.clone(),
            Some(other) => vec![other.clone()],
            None => Vec::new(),
        };
        Ok(Page {
            resources: inherited.resources,
            contents,
            media_box: inherited.media_box,
            rotate: inherited.rotate,
        })
    }
}

struct Inherited {
    resources: Option<Rc<Object>>,
    media_box: [f64; 4],
    rotate: i64,
}

impl Default for Inherited {
    fn default() -> Self {
        Self { resources: None, media_box: [0.0, 0.0, 612.0, 792.0], rotate: 0 }
    }
}
