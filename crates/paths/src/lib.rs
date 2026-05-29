use std::{
    borrow::Borrow,
    ffi::OsStr,
    fmt, ops,
    path::{Path, PathBuf},
};

pub use camino::{Utf8Component, Utf8Components, Utf8Path, Utf8PathBuf, Utf8Prefix};

#[derive(Debug, Clone, Ord, PartialOrd, Eq, Hash)]
pub struct AbsPathBuf(Utf8PathBuf);

impl From<AbsPathBuf> for Utf8PathBuf {
    fn from(AbsPathBuf(path_buf): AbsPathBuf) -> Utf8PathBuf {
        path_buf
    }
}

impl From<AbsPathBuf> for PathBuf {
    fn from(AbsPathBuf(path_buf): AbsPathBuf) -> PathBuf {
        path_buf.into()
    }
}

impl ops::Deref for AbsPathBuf {
    type Target = AbsPath;
    fn deref(&self) -> &AbsPath {
        self.as_path()
    }
}

impl AsRef<Utf8Path> for AbsPathBuf {
    fn as_ref(&self) -> &Utf8Path {
        self.0.as_path()
    }
}

impl AsRef<OsStr> for AbsPathBuf {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

impl AsRef<Path> for AbsPathBuf {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl AsRef<AbsPath> for AbsPathBuf {
    fn as_ref(&self) -> &AbsPath {
        self.as_path()
    }
}

impl Borrow<AbsPath> for AbsPathBuf {
    fn borrow(&self) -> &AbsPath {
        self.as_path()
    }
}

impl TryFrom<Utf8PathBuf> for AbsPathBuf {
    type Error = Utf8PathBuf;
    fn try_from(path_buf: Utf8PathBuf) -> Result<AbsPathBuf, Utf8PathBuf> {
        if !path_buf.is_absolute() {
            return Err(path_buf);
        }
        Ok(AbsPathBuf(path_buf))
    }
}

impl TryFrom<&str> for AbsPathBuf {
    type Error = Utf8PathBuf;
    fn try_from(path: &str) -> Result<AbsPathBuf, Utf8PathBuf> {
        AbsPathBuf::try_from(Utf8PathBuf::from(path))
    }
}

impl<P: AsRef<Path> + ?Sized> PartialEq<P> for AbsPathBuf {
    fn eq(&self, other: &P) -> bool {
        self.0.as_std_path() == other.as_ref()
    }
}

impl AbsPathBuf {
    pub fn assert(path: Utf8PathBuf) -> AbsPathBuf {
        AbsPathBuf::try_from(path)
            .unwrap_or_else(|path| panic!("expected absolute path, got {path}"))
    }

    pub fn assert_utf8(path: PathBuf) -> AbsPathBuf {
        AbsPathBuf::assert(
            Utf8PathBuf::from_path_buf(path)
                .unwrap_or_else(|path| panic!("expected utf8 path, got {}", path.display())),
        )
    }

    pub fn as_path(&self) -> &AbsPath {
        AbsPath::assert(self.0.as_path())
    }

    pub fn pop(&mut self) -> bool {
        self.0.pop()
    }

    pub fn push<P: AsRef<Utf8Path>>(&mut self, suffix: P) {
        self.0.push(suffix)
    }

    pub fn join(&self, path: impl AsRef<Utf8Path>) -> Self {
        Self(self.0.join(path))
    }
}

impl fmt::Display for AbsPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[derive(Debug, Ord, PartialOrd, Eq, Hash)]
#[repr(transparent)]
pub struct AbsPath(Utf8Path);

impl<P: AsRef<Path> + ?Sized> PartialEq<P> for AbsPath {
    fn eq(&self, other: &P) -> bool {
        self.0.as_std_path() == other.as_ref()
    }
}

impl AsRef<Utf8Path> for AbsPath {
    fn as_ref(&self) -> &Utf8Path {
        &self.0
    }
}

impl AsRef<Path> for AbsPath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl AsRef<OsStr> for AbsPath {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

impl ToOwned for AbsPath {
    type Owned = AbsPathBuf;

    fn to_owned(&self) -> Self::Owned {
        AbsPathBuf(self.0.to_owned())
    }
}

impl<'a> TryFrom<&'a Utf8Path> for &'a AbsPath {
    type Error = &'a Utf8Path;
    fn try_from(path: &'a Utf8Path) -> Result<&'a AbsPath, &'a Utf8Path> {
        if !path.is_absolute() {
            return Err(path);
        }
        Ok(AbsPath::assert(path))
    }
}

impl AbsPath {
    pub fn assert(path: &Utf8Path) -> &AbsPath {
        assert!(path.is_absolute(), "{path} is not absolute");
        unsafe { &*(path as *const Utf8Path as *const AbsPath) }
    }

    pub fn parent(&self) -> Option<&AbsPath> {
        self.0.parent().map(AbsPath::assert)
    }

    pub fn absolutize(&self, path: impl AsRef<Utf8Path>) -> AbsPathBuf {
        self.join(path).normalize()
    }

    pub fn join(&self, path: impl AsRef<Utf8Path>) -> AbsPathBuf {
        Utf8Path::join(self.as_ref(), path).try_into().unwrap()
    }

    pub fn normalize(&self) -> AbsPathBuf {
        AbsPathBuf(normalize_path(&self.0))
    }

    pub fn to_path_buf(&self) -> AbsPathBuf {
        AbsPathBuf::try_from(self.0.to_path_buf()).unwrap()
    }

    pub fn canonicalize(&self) -> ! {
        panic!(
            "We explicitly do not provide canonicalization API, as that is almost always a wrong solution, see #14430"
        )
    }

    pub fn strip_prefix(&self, base: &AbsPath) -> Option<&RelPath> {
        self.0.strip_prefix(base).ok().map(RelPath::new_unchecked)
    }
    pub fn starts_with(&self, base: &AbsPath) -> bool {
        self.0.starts_with(&base.0)
    }
    pub fn ends_with(&self, suffix: &RelPath) -> bool {
        self.0.ends_with(&suffix.0)
    }

    pub fn name_and_extension(&self) -> Option<(&str, Option<&str>)> {
        Some((self.file_stem()?, self.extension()))
    }

    pub fn file_name(&self) -> Option<&str> {
        self.0.file_name()
    }
    pub fn extension(&self) -> Option<&str> {
        self.0.extension()
    }
    pub fn file_stem(&self) -> Option<&str> {
        self.0.file_stem()
    }
    pub fn as_os_str(&self) -> &OsStr {
        self.0.as_os_str()
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    #[deprecated(note = "use Display instead")]
    pub fn display(&self) -> ! {
        unimplemented!()
    }
    #[deprecated(note = "use std::fs::metadata().is_ok() instead")]
    pub fn exists(&self) -> ! {
        unimplemented!()
    }

    pub fn components(&self) -> Utf8Components<'_> {
        self.0.components()
    }
}

impl fmt::Display for AbsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct RelPathBuf(Utf8PathBuf);

impl From<RelPathBuf> for Utf8PathBuf {
    fn from(RelPathBuf(path_buf): RelPathBuf) -> Utf8PathBuf {
        path_buf
    }
}

impl ops::Deref for RelPathBuf {
    type Target = RelPath;
    fn deref(&self) -> &RelPath {
        self.as_path()
    }
}

impl AsRef<Utf8Path> for RelPathBuf {
    fn as_ref(&self) -> &Utf8Path {
        self.0.as_path()
    }
}

impl AsRef<Path> for RelPathBuf {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl TryFrom<Utf8PathBuf> for RelPathBuf {
    type Error = Utf8PathBuf;
    fn try_from(path_buf: Utf8PathBuf) -> Result<RelPathBuf, Utf8PathBuf> {
        if !path_buf.is_relative() {
            return Err(path_buf);
        }
        Ok(RelPathBuf(path_buf))
    }
}

impl TryFrom<&str> for RelPathBuf {
    type Error = Utf8PathBuf;
    fn try_from(path: &str) -> Result<RelPathBuf, Utf8PathBuf> {
        RelPathBuf::try_from(Utf8PathBuf::from(path))
    }
}

impl RelPathBuf {
    pub fn as_path(&self) -> &RelPath {
        RelPath::new_unchecked(self.0.as_path())
    }
}

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct RelPath(Utf8Path);

impl AsRef<Utf8Path> for RelPath {
    fn as_ref(&self) -> &Utf8Path {
        &self.0
    }
}

impl AsRef<Path> for RelPath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl RelPath {
    pub fn new_unchecked(path: &Utf8Path) -> &RelPath {
        unsafe { &*(path as *const Utf8Path as *const RelPath) }
    }

    pub fn to_path_buf(&self) -> RelPathBuf {
        RelPathBuf::try_from(self.0.to_path_buf()).unwrap()
    }

    pub fn as_utf8_path(&self) -> &Utf8Path {
        self.as_ref()
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

fn normalize_path(path: &Utf8Path) -> Utf8PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Utf8Component::Prefix(..)) = components.peek().copied() {
        components.next();
        Utf8PathBuf::from(c.as_str())
    } else {
        Utf8PathBuf::new()
    };

    for component in components {
        match component {
            Utf8Component::Prefix(..) => unreachable!(),
            Utf8Component::RootDir => {
                ret.push(component.as_str());
            }
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                ret.pop();
            }
            Utf8Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}
