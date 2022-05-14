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

    fn n_to_m(n: usize, m: usize) {
        let mut r = <Anellus<usize>>::new(n+m); // need enough capacity for the poison pills
        let stock : usize = 100; 
        use std::thread::*;

        let mut producers: Vec<JoinHandle<usize>> = Vec::new();
        let mut consumers: Vec<JoinHandle<usize>> = Vec::new();
        
        for _t in 0..n {
            let mut rr = r.clone();
            producers.push(spawn(move || -> usize {
                let mut i = 1;
                while i <= stock {
                    i = match rr.push(i) {
                        Ok(_) => i + 1,
                        Err(_) => i,
                    };
                    yield_now();
                }
                0
            }));
        }

        for _t in 0..m {
            let rr = r.clone();
            consumers.push(spawn(move || -> usize {
                let mut count: usize = 0;
                loop {
                    match rr.pull() {
                        Ok(x) => {
                            if 0 == x {
                                return count;
                            }
                            count += 1;
                        }
                        Err(_) => {},
                    };
                    yield_now();
                }
            }));
        }

        for t in producers {
            t.join().unwrap();
        }
        for _i in 0..m {
            r.push(0).unwrap();
        }
        let mut totalseen = 0;
        for t in consumers {
            totalseen += t.join().unwrap();
        }

        assert_eq!(totalseen, stock*n);
    }

    #[test]
    fn ten_to_two() {
        n_to_m(10,2);
    }


    #[test]
    fn one_to_one() {
        n_to_m(1,1);
    }

    #[test]
    fn many_to_one() {
        n_to_m(4,1);
    }


    #[test]
    fn many_to_many() {
        n_to_m(10,5);
    }

    #[test]
    fn lots_producers() {
        n_to_m(100,2);
    }

    #[test]
    fn lots_consumers() {
        n_to_m(2,100);
    }

    #[test]
    fn lots_both() {
        n_to_m(1000,1000);
    }
}
