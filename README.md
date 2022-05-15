# Lockfree bounded MPMC queue

This repo started as small demo of a thread safe bounded multi producer multi consumer queue without mutexes to illustrate a thread on discord. However I decided to go for the extra challenge of using static allocated storage and it turned into a bit of a saga. 

## Bugs

After writing the stress-ish tests it became evident the implementation suffers from linerization violations. Some debug later I found the issue isn't swaped values but stale reads, sometimes the consumer gets an older version of the value in the buffer cell being read. Inspecting the cell however shows the correct value. 
The first hipothesis was stale from the cache on a different core. Playing with code timings it became evident it's not the cache. But that made me look at the code and spot the actual bug.

```
inner.w.compare_exchange(prevw, neww, inner.ordering, Ordering::Relaxed)
inner.ring[prevw] = value
```

```
value = inner.ring[newr]
inner.r.compare_exchange(prevr, newr, inner.ordering, Ordering::Relaxed)
```

There's a window between update of the write pointer and writting the value to buffer where a stale read can happen, and sometimes happens. 

## Fixes

Any change to the produce or consume instructions order will result in data loss. The naive fix is adding a write guard and make write something like 

```
inner.w.compare_exchange(prevw, neww, inner.ordering, Ordering::Relaxed)
inner.ring[prevw] = value
inner.guard.store(neww, inner.ordering)
```

and then check `w == guard` on read. However that will still fail if the reader loads the values just before the writer writes and results in a read deadlock if two threads concurrently store the guard and it gets stale. The guard algorithm can be made more complex untill it becomes either a spinlock or a mutex, at which point the queue stops being lockfree. 

So, solutions ? The problem becomes trivial with dynamic allocation, `compare_exchange` the next pointer on the queue linked list. For this specific case however I couldn't find a solution in the literature so if you know one, open an issue!

