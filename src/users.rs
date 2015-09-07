extern crate libc;

use libc::{c_char, uid_t, gid_t, time_t};
use std::ptr::*;
use std::ffi::CString;

#[repr(C)]
struct c_passwd {
    pw_name: *const c_char,
    pw_passwd: *const c_char,
    pw_uid: uid_t,
    pw_gid: gid_t,
    pw_change: time_t,
    pw_class: *const c_char,
    pw_gecos: *const c_char,
    pw_dir: *const c_char,
    pw_shell: *const c_char,
    pw_expire: time_t
}

#[repr(C)]
pub struct c_group {
    gr_name: *const c_char,
    gr_passwd: *const c_char,
    gr_gid: gid_t,
    gr_mem: *const *const c_char
}

extern {
    fn getpwnam(user_name: *const c_char) -> *const c_passwd;
    fn getgrnam(group_name: *const c_char) -> *const c_group;
}

pub enum Error {
    DUNNOLOL
}

use self::Error::*;

fn username_to_uid(s: &str) -> Result<u32, Error> {
    let c_name = CString::new(s).unwrap().as_ptr();
    unsafe {
        let pw = getpwnam(c_name);

        if pw.is_null() {
            return Err(DUNNOLOL)
        } else {
            return Ok(read(pw).pw_uid)
        }
    }
}

fn groupname_to_gid(s: &str) -> Result<u32, Error> {
    let c_name = CString::new(s).unwrap().as_ptr();
    unsafe {
        let pw = getgrnam(c_name);

        if pw.is_null() {
            return Err(DUNNOLOL)
        } else {
            return Ok(read(pw).gr_gid)
        }
    }

}

pub fn get_ids(user: &str, group: &str) -> Result<(u32,u32), Error> {
    match (username_to_uid(user), groupname_to_gid(group)) {
        (Ok(uid), Ok(gid)) => return Ok((uid,gid)),
        (_,_) => return Err(DUNNOLOL)
    }
}
