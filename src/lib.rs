use core::sync::atomic::{AtomicUsize, Ordering};
use std::cell::UnsafeCell;

#[derive(Debug)]
struct AnellusInner<T: Copy> 
{
    r: AtomicUsize,
    w: AtomicUsize,
    capacity: usize,
    ring: Vec<T>,
}

unsafe impl<T: Copy + Sync + Send> Sync for Anellus<T> {}
unsafe impl<T: Copy + Sync + Send> Send for Anellus<T> {}

#[derive(Debug)]
pub struct Anellus<T: Copy> {
    ptr: *mut AnellusInner<T>,
}

#[derive(Debug)]
pub enum Errors {
    Empty,
    Full,
}

type Result<T> = std::result::Result<T,Errors>;

//
// | .. | .. | .. | .. | .. | .. |
//   ^r        ^w
//
//              ^w  ^r
//

impl<T: Copy> Clone for Anellus<T> {
    fn clone(&self) -> Self {
        Anellus { ptr: self.ptr }
    }
}

impl<T: Copy> Anellus<T> {
    pub fn new(size: usize) -> Self {
        let mut res = Box::new(AnellusInner {
            r: AtomicUsize::new(0),
            w: AtomicUsize::new(1),
            capacity: size+2,
            ring: Vec::with_capacity(size+2)
        });
        unsafe {
            res.ring.reserve_exact(res.capacity);
            res.ring.set_len(res.capacity);
        }
        Anellus { ptr: Box::into_raw(res) }
    }

    pub fn pull(&self) -> Result<T> {
        let inner = unsafe { self.ptr.as_ref().unwrap() };
        let mut value: T;
        loop {
            let prevr = inner.r.load(Ordering::Relaxed);
            let prevw = inner.w.load(Ordering::Relaxed);
            let newr = (prevr + 1) % inner.capacity;
            if newr == prevw {
                return Err(Errors::Empty);
            }
            value = inner.ring[newr];
            match inner.r.compare_exchange(prevr, newr, Ordering::SeqCst, Ordering::Relaxed) {
                Ok(_) => break,
                Err(_) => {},
            }
        }
            Ok(value)
    }

    pub fn push(&mut self, value: T) -> Result<()> {
        let inner = unsafe { self.ptr.as_mut().unwrap() };
        loop {
            let prevr = inner.r.load(Ordering::Relaxed);
            let prevw = inner.w.load(Ordering::Relaxed);
            let neww = (prevw + 1) % inner.capacity;
            if neww == prevr {
                return Err(Errors::Full);
            }
            match inner.w.compare_exchange(prevw, neww, Ordering::SeqCst, Ordering::Relaxed) {
                Ok(_) => { 
                    inner.ring[prevw] = value;
                    break;
                },
                Err(_) => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Anellus;
    
    #[test]
    fn read_from_empty() {
        let r : Anellus<u32> = Anellus::new(3); 
   
        assert!(r.pull().is_err());
    }

    #[test]
    fn basic_write() {
        let mut r : Anellus<u32> = Anellus::new(3); 

        assert!(r.push(1).is_ok());
    }

    #[test]
    fn write_to_full() {
        let mut r : Anellus<u32> = Anellus::new(3); 
    
        assert!(r.push(1).is_ok());
        assert!(r.push(2).is_ok());
        assert!(r.push(3).is_ok());
        assert!(r.push(4).is_err());
    }

    #[test]
    fn can_pull() {
        let mut r : Anellus<u32> = Anellus::new(3); 
    
        r.push(1);
        r.push(2);
        match r.pull() {
            Ok(x) => assert_eq!(x,1),
            Err(x) => panic!("{:?}", x),
        }
        assert!(r.push(3).is_ok());
        assert!(r.push(4).is_ok());
        match r.pull() {
            Ok(x) => assert_eq!(x,2),
            Err(x) => panic!("{:?}", x),
        }
        match r.pull() {
            Ok(x) => assert_eq!(x,3),
            Err(x) => panic!("{:?}", x),
        }
        match r.pull() {
            Ok(x) => assert_eq!(x,4),
            Err(x) => panic!("{:?}", x),
        }
        
        assert!(r.pull().is_err());
    }

    #[test]
    fn one_to_one() {
        let r = <Anellus<u32>>::new(10); 
        
        use std::thread;

        let mut p1 = r.clone();
        let mut c = r.clone();
        

        let prod1 = thread::spawn(move || {
            let mut i = 1;
            while i < 1001 {
                i = match p1.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
            }
        });

        let con = thread::spawn(move || {
            let mut count: u32;
            count = 1;
            while count < 1000 {
                count = match c.pull() {
                    Ok(_) => count + 1,
                    Err(x) => count,
                };
                thread::yield_now();
            }
        });

        prod1.join().unwrap();
        let res = con.join();

        assert!(res.is_ok());
    }

    #[test]
    fn many_to_one() {
        let r = <Anellus<u32>>::new(10); 
        
        use std::thread;

        let mut p1 = r.clone();
        let mut p2 = r.clone();
        let mut p3 = r.clone();
        let mut p4 = r.clone();
        let mut c = r.clone();
        

        let prod1 = thread::spawn(move || {
            let mut i = 0;
            while i < 100 {
                i = match p1.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
               // println!(">>> {:?}",i);
            }
        });
        let prod2 = thread::spawn(move || {
            let mut i = 0;
            while i < 100 {
                i = match p2.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
                //println!(">>> {:?}",i);
            }
        });
        let prod3 = thread::spawn(move || {
            let mut i = 0;
            while i < 100 {
                i = match p3.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
                //println!(">>> {:?}",i);
            }
        });
        let prod4 = thread::spawn(move || {
            let mut i = 0;
            while i < 100 {
                i = match p4.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
                //println!(">>> {:?}",i);
            }
        });

        let con = thread::spawn(move || {
            let mut count: u32;
            count = 0;
            while count < 400 {
                count = match c.pull() {
                    Ok(_) => count + 1,
                    Err(x) => count,
                };
                thread::yield_now();
                //println!("<< {:?}", count);
            }
        });

        prod1.join().unwrap();
        prod2.join().unwrap();
        prod3.join().unwrap();
        prod4.join().unwrap();
        let res = con.join();

        assert!(res.is_ok());
    }


    #[test]
    fn many_to_many() {
        let r = <Anellus<u32>>::new(10); 
        
        use std::thread;

        let mut p1 = r.clone();
        let mut p2 = r.clone();
        let mut p3 = r.clone();
        let mut p4 = r.clone();
        let mut c1 = r.clone();
        let mut c2 = r.clone();
        

        let prod1 = thread::spawn(move || {
            let mut i = 0;
            while i < 100 {
                i = match p1.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
            }
        });
        let prod2 = thread::spawn(move || {
            let mut i = 0;
            while i < 100 {
                i = match p2.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
            }
        });
        let prod3 = thread::spawn(move || {
            let mut i = 0;
            while i < 100 {
                i = match p3.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
            }
        });
        let prod4 = thread::spawn(move || {
            let mut i = 0;
            while i < 100 {
                i = match p4.push(i) {
                    Ok(_) => i + 1,
                    Err(_) => i,
                };
                thread::yield_now();
            }
        });

        let con1 = thread::spawn(move || {
            let mut count: u32;
            count = 0;
            while count < 200 {
                count = match c1.pull() {
                    Ok(_) => count + 1,
                    Err(x) => count,
                };
                thread::yield_now();
            }
        });

        let con2 = thread::spawn(move || {
            let mut count: u32;
            count = 0;
            while count < 200 {
                count = match c2.pull() {
                    Ok(_) => count + 1,
                    Err(x) => count,
                };
                thread::yield_now();
            }
        });

        prod1.join().unwrap();
        prod2.join().unwrap();
        prod3.join().unwrap();
        prod4.join().unwrap();
        let res1 = con1.join();
        let res2 = con2.join();

        assert!(res1.is_ok());
        assert!(res2.is_ok());
    }
}
