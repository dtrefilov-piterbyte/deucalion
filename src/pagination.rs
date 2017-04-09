
pub trait PaginatedRequestor {
    type Item: 'static + Clone;
    type Error: 'static;
    fn next_page(&mut self) -> Result<Option<Vec<Self::Item>>, Self::Error>;
}

pub struct PaginatedIterator<'a, TR: PaginatedRequestor> {
    requestor: TR,
    current_page: Option<Vec<TR::Item>>,
    error: &'a mut Option<TR::Error>
}

impl<'a, TR: PaginatedRequestor> PaginatedIterator<'a, TR> {
    pub fn new(requestor: TR, error: &'a mut Option<TR::Error>) -> Self {
        PaginatedIterator {
            requestor: requestor,
            current_page: None,
            error: error
        }
    }

    fn advance_page(&mut self) {
        self.current_page = match self.requestor.next_page() {
            Ok(Some(p)) => Some(p.iter().cloned().rev().collect()),
            Ok(None) => None,
            Err(e) => { *self.error = Some(e); None }
        }
    }
}

impl<'a, TR: PaginatedRequestor> Iterator for PaginatedIterator<'a, TR> {
    type Item = TR::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_page.is_none() {
            self.advance_page();
            if self.current_page.is_none() {
                return None;
            }
        }
        match self.current_page.as_mut().unwrap().pop() {
            Some(i) => Some(i),
            None => {
                self.advance_page();
                match self.current_page {
                    Some(_) => self.next(),
                    None => None
                }
            }
        }
    }
}
