#![feature(test)]
extern crate test;

use core::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

#[derive(Debug)]
struct AnellusInner<T: Copy> 
{
    r: AtomicUsize,
    w: AtomicUsize,
    ordering: Ordering,
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
            ordering: Ordering::SeqCst,
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
            match inner.r.compare_exchange(prevr, newr, inner.ordering, Ordering::Relaxed) {
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
            match inner.w.compare_exchange(prevw, neww, inner.ordering, Ordering::Relaxed) {
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
    
        r.push(1).unwrap();
        r.push(2).unwrap();
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

    fn n_to_m(n: u16, m: u16) {
        let mut r = <Anellus<u64>>::new((m+n+32) as usize); // need enough capacity for the poison pills
        let stock : u32 = 100; 
        use std::thread::*;

        let mut producers: Vec<JoinHandle<u64>> = Vec::new();
        let mut consumers: Vec<JoinHandle<Vec<u64>>> = Vec::new();
        
        for t in 0..n {
            let mut rr = r.clone();
            producers.push(spawn(move || -> u64 {
                let mut i : u32 = 1;
                while i <= stock {
                    i = match rr.push(((t as u64)<<32) + (i as u64)) {
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
            consumers.push(spawn(move || -> Vec<u64> {
                let mut values : Vec<u64> = Vec::new();
                loop {
                    match rr.pull() {
                        Ok(x) => {
                            if 0 == x {
                                return values;
                            }
                            values.push(x);
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
        let mut seenvalues : Vec<Vec<u64>> = Vec::new();
        for t in consumers {
            seenvalues.push(t.join().unwrap());
        }
        
        let mut seenoneach : Vec<u32> = vec![0; n.into()];
        for values in seenvalues {
            let mut lastforeach: Vec<u32> = vec![0; n.into()];
            for v in values {
                let producer : usize = ((v >> 32) as u32).try_into().unwrap();
                let counter : u32 = (v % (u32::MAX as u64)).try_into().unwrap();
                seenoneach[producer] += 1;
                if counter <= lastforeach[producer] {
                    panic!("order violation for {} was {} now {}", producer, lastforeach[producer], counter);
                }
                lastforeach[producer] = counter;
            }
        }

        let mut totalseen = 0;
        for t in 0..n {
            //println!("producer {} : {}", t, seenoneach[t as usize]);
            totalseen += seenoneach[t as usize];
        }

        assert_eq!(totalseen, (stock as u32)*(n as u32));
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

    #[test]
    fn ddos() {
        n_to_m(100,1);
    }

    use super::*;
    use test::Bencher;
    #[allow(soft_unstable)]
    #[bench]
    fn strict_order(b: &mut Bencher) {
        b.iter(|| n_to_m(1000,1));
    }
}
