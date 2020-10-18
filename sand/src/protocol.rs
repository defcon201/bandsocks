// The protocol is defined here canonically and then imported
// by the runtime crate along with our finished binary.

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[repr(C)]
pub struct MessageToSand {
    pub task: VPid,
    pub op: ToSand,
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[repr(C)]
pub struct MessageFromSand {
    pub task: VPid,
    pub op: FromSand,
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[repr(C)]
pub enum ToSand {
    OpenProcessReply,
    SysOpenReply(Result<SysFd, Errno>),
    SysAccessReply(Result<(), Errno>),
    SysKillReply(Result<(), Errno>),
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[repr(C)]
pub enum FromSand {
    OpenProcess(SysPid),
    SysAccess(SysAccess),
    SysOpen(SysAccess, i32),
    SysKill(VPid, Signal),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, Hash32, Serialize, Deserialize)]
#[repr(C)]
pub struct SysPid(pub u32);

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, Hash32, Serialize, Deserialize)]
#[repr(C)]
pub struct VPid(pub u32);

#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct Signal(pub u32);

#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct VPtr(pub usize);

#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct VString(pub VPtr);
#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct Errno(pub i32);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SysFd(pub u32);

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[repr(C)]
pub struct SysAccess {
    pub dir: Option<SysFd>,
    pub path: VString,
    pub mode: i32,
}

pub mod buffer {
    use super::{de, ser, SysFd};
    use core::fmt;
    use heapless::{consts::*, Vec};
    use serde::{de::DeserializeOwned, Serialize};

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum Error {
        Unimplemented,
        UnexpectedEnd,
        BufferFull,
        InvalidValue,
        Serialize,
        Deserialize,
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{:?}", self)
        }
    }

    pub type Result<T> = core::result::Result<T, Error>;
    pub type BytesMax = U128;
    pub type FilesMax = U8;

    #[derive(Default)]
    pub struct IPCBuffer {
        bytes: Vec<u8, BytesMax>,
        files: Vec<SysFd, FilesMax>,
        byte_offset: usize,
        file_offset: usize,
    }

    #[derive(Debug, Eq, PartialEq)]
    pub struct IPCSlice<'a> {
        pub bytes: &'a [u8],
        pub files: &'a [SysFd],
    }

    #[derive(Debug, Eq, PartialEq)]
    pub struct IPCSliceMut<'a> {
        pub bytes: &'a mut [u8],
        pub files: &'a mut [SysFd],
    }

    impl<'a> IPCBuffer {
        pub fn new() -> Self {
            Default::default()
        }

        pub fn reset(&mut self) {
            self.bytes.clear();
            self.files.clear();
            self.byte_offset = 0;
            self.file_offset = 0;
        }

        pub fn byte_capacity(&self) -> usize {
            self.bytes.capacity()
        }

        pub unsafe fn set_len(&mut self, num_bytes: usize, num_files: usize) {
            assert_eq!(self.byte_offset, 0);
            assert_eq!(self.file_offset, 0);
            assert!(num_bytes <= self.bytes.capacity());
            assert!(num_files <= self.files.capacity());
            self.bytes.set_len(num_bytes);
            self.files.set_len(num_files);
        }

        pub fn is_empty(&self) -> bool {
            self.byte_offset == self.bytes.len() && self.file_offset == self.files.len()
        }

        pub fn as_slice(&'a self) -> IPCSlice<'a> {
            IPCSlice {
                bytes: &self.bytes[self.byte_offset..],
                files: &self.files[self.file_offset..],
            }
        }

        pub fn as_slice_mut(&'a mut self) -> IPCSliceMut<'a> {
            IPCSliceMut {
                bytes: &mut self.bytes[self.byte_offset..],
                files: &mut self.files[self.file_offset..],
            }
        }

        pub fn push_back<T: Serialize>(&mut self, message: &T) -> Result<()> {
            let mut serializer = ser::IPCSerializer { output: self };
            message.serialize(&mut serializer)
        }

        pub fn pop_front<T: Clone + DeserializeOwned>(&'a mut self) -> Result<T> {
            let mut deserializer = de::IPCDeserializer { input: self };
            T::deserialize(&mut deserializer)
        }

        pub fn extend_bytes(&mut self, data: &[u8]) -> Result<()> {
            self.bytes
                .extend_from_slice(data)
                .map_err(|_| Error::BufferFull)
        }

        pub fn push_back_byte(&mut self, data: u8) -> Result<()> {
            self.bytes.push(data).map_err(|_| Error::BufferFull)
        }

        pub fn push_back_file(&mut self, file: SysFd) -> Result<()> {
            self.files.push(file).map_err(|_| Error::BufferFull)
        }

        pub fn front_bytes(&self, len: usize) -> Result<&[u8]> {
            let bytes = self.as_slice().bytes;
            if len <= bytes.len() {
                Ok(&bytes[..len])
            } else {
                Err(Error::UnexpectedEnd)
            }
        }

        pub fn front_files(&self, len: usize) -> Result<&[SysFd]> {
            let files = self.as_slice().files;
            if len <= files.len() {
                Ok(&files[..len])
            } else {
                Err(Error::UnexpectedEnd)
            }
        }

        pub fn pop_front_bytes(&mut self, len: usize) {
            let new_offset = self.byte_offset + len;
            assert!(new_offset <= self.bytes.len());
            self.byte_offset = new_offset;
        }

        pub fn pop_front_files(&mut self, len: usize) {
            let new_offset = self.file_offset + len;
            assert!(new_offset <= self.files.len());
            self.file_offset = new_offset;
        }

        pub fn pop_front_file(&mut self) -> Result<SysFd> {
            let result = self.front_files(1).map(|slice| slice[0].clone());
            if result.is_ok() {
                self.pop_front_files(1);
            }
            result
        }
    }
}

mod ser {
    use super::{
        buffer::{Error, IPCBuffer, Result},
        SysFd,
    };
    use core::{fmt::Display, result};
    use serde::{ser, ser::SerializeTupleStruct};

    const SYSFD: &str = "fd@ser";

    pub struct IPCSerializer<'a> {
        pub output: &'a mut IPCBuffer,
    }

    impl ser::Serialize for SysFd {
        fn serialize<S: ser::Serializer>(&self, serializer: S) -> result::Result<S::Ok, S::Error> {
            let mut tuple = serializer.serialize_tuple_struct(SYSFD, 1)?;
            tuple.serialize_field(&self.0)?;
            tuple.end()
        }
    }

    impl ser::StdError for Error {}

    impl ser::Error for Error {
        fn custom<T: Display>(_msg: T) -> Self {
            Error::Serialize
        }
    }

    impl<'a, 'b> ser::Serializer for &'b mut IPCSerializer<'a> {
        type Ok = ();
        type Error = Error;
        type SerializeSeq = Self;
        type SerializeTuple = Self;
        type SerializeTupleStruct = Self;
        type SerializeTupleVariant = Self;
        type SerializeMap = Self;
        type SerializeStruct = Self;
        type SerializeStructVariant = Self;

        fn is_human_readable(&self) -> bool {
            false
        }

        fn collect_str<T: ?Sized + Display>(self, v: &T) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_bool(self, v: bool) -> Result<()> {
            self.output.push_back_byte(v as u8)
        }

        fn serialize_f32(self, v: f32) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_f64(self, v: f64) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_i16(self, v: i16) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_i32(self, v: i32) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_i64(self, v: i64) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_i8(self, v: i8) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_none(self) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_some<T: ?Sized + ser::Serialize>(self, v: &T) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_u16(self, v: u16) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_u64(self, v: u64) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_u8(self, v: u8) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_unit(self) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_unit_struct(self, name: &'static str) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_unit_variant(
            self,
            name: &'static str,
            varidx: u32,
            var: &'static str,
        ) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_char(self, _v: char) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_str(self, _v: &str) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_bytes(self, _v: &[u8]) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_u32(mut self, v: u32) -> Result<()> {
            Err(Error::Unimplemented)
        }

        fn serialize_tuple_struct(self, name: &'static str, len: usize) -> Result<Self> {
            Err(Error::Unimplemented)
        }

        fn serialize_newtype_struct<T>(self, _: &'static str, value: &T) -> Result<()>
        where
            T: ?Sized + ser::Serialize,
        {
            value.serialize(self)
        }

        fn serialize_newtype_variant<T>(
            self,
            name: &'static str,
            variant_index: u32,
            variant: &'static str,
            value: &T,
        ) -> Result<()>
        where
            T: ?Sized + ser::Serialize,
        {
            if variant_index < 0x100 {
                self.output.push_back_byte(variant_index as u8);
                value.serialize(self)
            } else {
                Err(Error::InvalidValue)
            }
        }

        fn serialize_seq(self, len: Option<usize>) -> Result<Self> {
            Err(Error::Unimplemented)
        }

        fn serialize_tuple(self, len: usize) -> Result<Self> {
            Err(Error::Unimplemented)
        }

        fn serialize_map(self, len: Option<usize>) -> Result<Self> {
            Err(Error::Unimplemented)
        }

        fn serialize_struct(self, name: &'static str, _len: usize) -> Result<Self> {
            Err(Error::Unimplemented)
        }

        fn serialize_tuple_variant(
            self,
            name: &'static str,
            variant_index: u32,
            variant: &'static str,
            len: usize,
        ) -> Result<Self> {
            Err(Error::Unimplemented)
        }

        fn serialize_struct_variant(
            self,
            name: &'static str,
            variant_index: u32,
            variant: &'static str,
            len: usize,
        ) -> Result<Self> {
            Err(Error::Unimplemented)
        }
    }

    impl<'a, 'b> ser::SerializeSeq for &'b mut IPCSerializer<'a> {
        type Ok = ();
        type Error = Error;

        fn serialize_element<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<()> {
            value.serialize(&mut **self)
        }

        fn end(self) -> Result<()> {
            Ok(())
        }
    }

    impl<'a, 'b> ser::SerializeTuple for &'b mut IPCSerializer<'a> {
        type Ok = ();
        type Error = Error;

        fn serialize_element<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<()> {
            value.serialize(&mut **self)
        }

        fn end(self) -> Result<()> {
            Ok(())
        }
    }

    impl<'a, 'b> ser::SerializeTupleStruct for &'b mut IPCSerializer<'a> {
        type Ok = ();
        type Error = Error;

        fn serialize_field<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<()> {
            value.serialize(&mut **self)
        }

        fn end(self) -> Result<()> {
            Ok(())
        }
    }

    impl<'a, 'b> ser::SerializeTupleVariant for &'b mut IPCSerializer<'a> {
        type Ok = ();
        type Error = Error;

        fn serialize_field<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<()> {
            value.serialize(&mut **self)
        }

        fn end(self) -> Result<()> {
            Ok(())
        }
    }

    impl<'a, 'b> ser::SerializeMap for &'b mut IPCSerializer<'a> {
        type Ok = ();
        type Error = Error;

        fn serialize_key<T: ?Sized + ser::Serialize>(&mut self, key: &T) -> Result<()> {
            key.serialize(&mut **self)
        }

        fn serialize_value<T: ?Sized + ser::Serialize>(&mut self, value: &T) -> Result<()> {
            value.serialize(&mut **self)
        }

        fn end(self) -> Result<()> {
            Ok(())
        }
    }
    impl<'a, 'b> ser::SerializeStruct for &'b mut IPCSerializer<'a> {
        type Ok = ();
        type Error = Error;

        fn serialize_field<T>(&mut self, _name: &'static str, value: &T) -> Result<()>
        where
            T: ?Sized + ser::Serialize,
        {
            value.serialize(&mut **self)
        }

        fn end(self) -> Result<()> {
            Ok(())
        }
    }

    impl<'a, 'b> ser::SerializeStructVariant for &'b mut IPCSerializer<'a> {
        type Ok = ();
        type Error = Error;

        fn serialize_field<T>(&mut self, _name: &'static str, value: &T) -> Result<()>
        where
            T: ?Sized + ser::Serialize,
        {
            value.serialize(&mut **self)
        }

        fn end(self) -> Result<()> {
            Ok(())
        }
    }
}

mod de {
    use super::{
        buffer::{Error, IPCBuffer, Result},
        SysFd,
    };
    use core::{fmt::Display, result};
    use serde::de;

    pub struct IPCDeserializer<'d> {
        pub input: &'d mut IPCBuffer,
    }

    impl<'d> de::Deserialize<'d> for SysFd {
        fn deserialize<D: de::Deserializer<'d>>(deserializer: D) -> result::Result<Self, D::Error> {
            println!("would deserialize a file here");
            Ok(SysFd(999))
        }
    }

    impl de::Error for Error {
        fn custom<T: Display>(_msg: T) -> Self {
            Error::Deserialize
        }
    }

    impl<'d, 'a> de::Deserializer<'d> for &'a mut IPCDeserializer<'d> {
        type Error = Error;

        fn is_human_readable(&self) -> bool {
            false
        }

        fn deserialize_any<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_bool<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_byte_buf<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_bytes<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_char<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_enum<V: de::Visitor<'d>>(
            self,
            name: &'static str,
            variants: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_f32<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_f64<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_i16<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_i32<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_i64<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_i8<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_identifier<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_ignored_any<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_map<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_newtype_struct<V: de::Visitor<'d>>(
            self,
            name: &'static str,
            visitor: V,
        ) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_option<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_seq<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_str<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_string<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_struct<V: de::Visitor<'d>>(
            self,
            name: &'static str,
            fields: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_tuple<V: de::Visitor<'d>>(self, len: usize, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_tuple_struct<V: de::Visitor<'d>>(
            self,
            name: &'static str,
            len: usize,
            visitor: V,
        ) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_u16<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_u32<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_u64<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_u8<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_unit<V: de::Visitor<'d>>(self, visitor: V) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }

        fn deserialize_unit_struct<V: de::Visitor<'d>>(
            self,
            name: &'static str,
            visitor: V,
        ) -> Result<V::Value> {
            Err(Error::Unimplemented)
        }
    }
}

#[cfg(test)]
mod test {
    use super::{
        buffer::{Error, IPCBuffer, IPCSlice},
        Errno, SysFd, VPtr,
    };

    #[test]
    fn u32() {
        let mut buf = IPCBuffer::new();
        buf.push_back(&0x12345678u32).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[0x78, 0x56, 0x34, 0x12],
                files: &[],
            }
        );
        assert_eq!(buf.pop_front::<u32>().unwrap(), 0x12345678);
        assert!(buf.is_empty());
    }

    #[test]
    fn u8() {
        let mut buf = IPCBuffer::new();
        buf.push_back(&0x42u8).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[0x42],
                files: &[],
            }
        );
        assert_eq!(buf.pop_front::<u8>().unwrap(), 0x42);
        assert!(buf.is_empty());
    }

    #[test]
    fn u64() {
        let mut buf = IPCBuffer::new();
        buf.push_back(&0x12345678abcdabbau64).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[0xba, 0xab, 0xcd, 0xab, 0x78, 0x56, 0x34, 0x12],
                files: &[],
            }
        );
        assert_eq!(buf.pop_front::<u64>().unwrap(), 0x12345678abcdabbau64);
        assert!(buf.is_empty());
    }

    #[test]
    fn i32() {
        let mut buf = IPCBuffer::new();
        buf.push_back(&-1i32).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[0xff, 0xff, 0xff, 0xff],
                files: &[],
            }
        );
        assert_eq!(buf.pop_front::<i32>().unwrap(), -1);
        assert!(buf.is_empty());
    }

    #[test]
    fn no_char() {
        let mut buf = IPCBuffer::new();
        assert_eq!(buf.push_back(&'ก'), Err(Error::Unimplemented));
    }

    #[test]
    fn no_str() {
        let mut buf = IPCBuffer::new();
        assert_eq!(buf.push_back(&"yo"), Err(Error::Unimplemented));
    }

    #[test]
    fn fixed_len_bytes() {
        let mut buf = IPCBuffer::new();
        let msg = (true, b"blah");
        buf.push_back(&msg).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[1, 98, 108, 97, 104],
                files: &[],
            }
        );
        let result = buf.pop_front::<(bool, [u8; 4])>().unwrap();
        assert_eq!(&result.1, msg.1);
        assert!(buf.is_empty());
    }

    #[test]
    fn vptr() {
        let mut buf = IPCBuffer::new();
        buf.push_back(&VPtr(0x1122334455667788)).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11],
                files: &[],
            }
        );
        assert_eq!(buf.pop_front::<VPtr>().unwrap(), VPtr(0x1122334455667788));
        assert!(buf.is_empty());
    }

    #[test]
    fn sysfd() {
        let mut buf = IPCBuffer::new();
        buf.push_back(&SysFd(0x87654321)).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[],
                files: &[SysFd(0x87654321)],
            }
        );
        assert_eq!(buf.pop_front::<SysFd>().unwrap(), SysFd(0x87654321));
        assert!(buf.is_empty());
    }

    #[test]
    fn sysfd_multi() {
        let mut buf = IPCBuffer::new();
        type T = [SysFd; 4];
        let msg: T = [SysFd(5), SysFd(6), SysFd(7), SysFd(8)];
        buf.push_back(&msg).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[],
                files: &msg,
            }
        );
        assert_eq!(buf.pop_front::<T>().unwrap(), msg);
        assert!(buf.is_empty());
    }

    #[test]
    fn sysfd_result_ok() {
        let mut buf = IPCBuffer::new();
        type T = Result<SysFd, Errno>;
        let msg: T = Ok(SysFd(0x12341122));
        buf.push_back(&msg).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[0],
                files: &[SysFd(0x12341122)],
            }
        );
        assert_eq!(buf.pop_front::<T>().unwrap(), msg);
        assert!(buf.is_empty());
    }

    #[test]
    fn sysfd_result_err() {
        let mut buf = IPCBuffer::new();
        type T = Result<SysFd, Errno>;
        let msg: T = Err(Errno(-1));
        buf.push_back(&msg).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[1, 0xff, 0xff, 0xff, 0xff],
                files: &[],
            }
        );
        assert_eq!(buf.pop_front::<T>().unwrap(), msg);
        assert!(buf.is_empty());
    }

    #[test]
    fn tuple() {
        let mut buf = IPCBuffer::new();
        let msg = (true, false, false, 0xabcdu16, 999.0f64);
        buf.push_back(&msg).unwrap();
        assert_eq!(
            buf.as_slice(),
            IPCSlice {
                bytes: &[1, 0, 0, 205, 171, 0, 0, 0, 0, 0, 56, 143, 64],
                files: &[],
            }
        );
        assert_eq!(
            buf.pop_front::<(bool, bool, bool, u16, f64)>().unwrap(),
            msg
        );
        assert!(buf.is_empty());
    }
}
