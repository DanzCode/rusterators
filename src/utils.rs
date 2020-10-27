use std::ops::{Deref, DerefMut};

/// Wraps a value that is about to be replaced by a value generated by a destructive operation on the value itself
/// Consider
/// ```
/// pub struct Container(Incremented);
///
/// pub struct Incremented(i32);
/// impl Incremented {
///     pub fn increment(self) -> Self {
///         Self (self.0+1)
///     }
/// }
/// ```
///
/// Container can't really use Incremented::increment on its attribute since it would need to move it into the method leaving its .0 attribute undefined
/// However since the operation is somewhat "semanticly atomic" (i.e. the attribute will be valid before and after method call) as long as Incremented::increment does not know about container and the container is not synced between theads, it is safe to assume that container.0 is always in a valid state
/// SelfUpdating simulates this by implementing a smartpointer over a value that may be updated by moving the original value out inside a passed closure by v.update():
///
/// ```
/// pub struct Container(SelfUpdating<Incremented>);
/// impl Container {
///     pub fn increment(&mut self) {
///         self.0.update(|i| i.increment());
///     }
/// }
/// ```
pub struct SelfUpdating<T>(Option<T>);

impl<T> SelfUpdating<T> {
    pub fn of(initial: T) -> Self {
        Self(Some(initial))
    }

    pub fn update<F: FnOnce(T) -> T>(&mut self, op: F) {
        self.0 = Some(op(self.0.take().unwrap()))
    }

    pub fn unwrap(mut self) -> T {self.0.take().unwrap()}
}

impl<T> From<T> for SelfUpdating<T> {
    fn from(r: T) -> Self {
        SelfUpdating::of(r)
    }
}
impl<T> Deref for SelfUpdating<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}

impl<T> DerefMut for SelfUpdating<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().unwrap()
    }
}

mod tests {
    use crate::utils::SelfUpdating;

    #[test]
    fn self_updating_init() {
        let self_updating=SelfUpdating::of(String::from("t"));
        match self_updating.0 {
            Some(v) => assert_eq!(v,"t"),
            _ => panic!("invalid state")
        }
    }

    #[test]
    fn self_updating_unwrap() {
        let self_updating=SelfUpdating::of(String::from("t"));
        assert_eq!(self_updating.unwrap(),"t")
    }

    #[test]
    fn self_updating_deref() {
        let self_updating=SelfUpdating::of(String::from("t"));
        assert_eq!(self_updating.len(),1);
    }

    #[test]
    fn self_updating_deref_mut() {
        let mut self_updating=SelfUpdating::of(String::from("test"));
        assert_eq!(self_updating.remove(0),'t');
    }
    #[test]
    fn self_updating_perform_update() {
        let mut self_updating=SelfUpdating::of(String::from("test"));
        self_updating.update(|s| s.repeat(2));
        assert_eq!(self_updating.unwrap(),"testtest");
    }


}