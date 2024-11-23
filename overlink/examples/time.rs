use std::time::SystemTime;

use overlink::overlink;

fn main() {
    println!("time outside: {:?}", SystemTime::now());
}

#[overlink]
unsafe extern "C" fn clock_gettime(
    clockid: libc::clockid_t,
    res: *mut libc::timespec,
) -> libc::c_int {
    println!("time inside: {:?}", SystemTime::now());

    let result = super!(clockid, res);

    if result != 0 {
        return result;
    }

    (*res).tv_sec += 3600;
    0
}
