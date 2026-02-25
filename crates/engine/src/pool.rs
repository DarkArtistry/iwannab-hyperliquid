//! Object pool for hot-path allocation reuse (Phase B5)
//!
//! Reduces allocation pressure by recycling Vec<Fill> and other
//! frequently-allocated objects on the matching engine hot path.

use std::sync::Mutex;

/// Thread-safe object pool for Vec<T>
///
/// Uses Mutex for Send + Sync compatibility. The matching engine runs
/// on a single dedicated thread so the mutex is never contended,
/// giving near-zero overhead while allowing the pool to be embedded
/// in types that must be Send + Sync (e.g. Engine inside AppState).
pub struct VecPool<T> {
    pool: Mutex<Vec<Vec<T>>>,
    max_pool_size: usize,
}

impl<T> VecPool<T> {
    /// Create a new pool with the given maximum cached vecs
    pub fn new(max_pool_size: usize) -> Self {
        Self {
            pool: Mutex::new(Vec::new()),
            max_pool_size,
        }
    }

    /// Get a Vec from the pool (or create a new one)
    #[inline]
    pub fn get(&self) -> Vec<T> {
        self.pool.lock().unwrap().pop().unwrap_or_default()
    }

    /// Get a Vec with pre-allocated capacity
    #[inline]
    pub fn get_with_capacity(&self, capacity: usize) -> Vec<T> {
        let mut v = self.get();
        if v.capacity() < capacity {
            v.reserve(capacity - v.len());
        }
        v
    }

    /// Return a Vec to the pool for reuse
    #[inline]
    pub fn put(&self, mut v: Vec<T>) {
        v.clear();
        let mut pool = self.pool.lock().unwrap();
        if pool.len() < self.max_pool_size {
            pool.push(v);
        }
        // Otherwise drop it - pool is full
    }

    /// Get the number of cached vecs
    pub fn cached_count(&self) -> usize {
        self.pool.lock().unwrap().len()
    }
}

/// Pooled wrapper that automatically returns Vec to pool on drop
pub struct PooledVec<'a, T> {
    vec: Option<Vec<T>>,
    pool: &'a VecPool<T>,
}

impl<'a, T> PooledVec<'a, T> {
    /// Create a new pooled vec from a pool
    #[inline]
    pub fn new(pool: &'a VecPool<T>) -> Self {
        Self {
            vec: Some(pool.get()),
            pool,
        }
    }

    /// Create with pre-allocated capacity
    #[inline]
    pub fn with_capacity(pool: &'a VecPool<T>, capacity: usize) -> Self {
        Self {
            vec: Some(pool.get_with_capacity(capacity)),
            pool,
        }
    }

    /// Take ownership of the inner Vec (prevents return to pool)
    #[inline]
    pub fn take(mut self) -> Vec<T> {
        self.vec.take().unwrap()
    }
}

impl<'a, T> std::ops::Deref for PooledVec<'a, T> {
    type Target = Vec<T>;

    #[inline]
    fn deref(&self) -> &Vec<T> {
        self.vec.as_ref().unwrap()
    }
}

impl<'a, T> std::ops::DerefMut for PooledVec<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Vec<T> {
        self.vec.as_mut().unwrap()
    }
}

impl<'a, T> Drop for PooledVec<'a, T> {
    fn drop(&mut self) {
        if let Some(v) = self.vec.take() {
            self.pool.put(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_pool_basic() {
        let pool = VecPool::<u32>::new(4);

        // Get from empty pool creates new
        let mut v = pool.get();
        assert!(v.is_empty());

        // Use it
        v.push(1);
        v.push(2);
        v.push(3);
        let cap = v.capacity();

        // Return to pool
        pool.put(v);
        assert_eq!(pool.cached_count(), 1);

        // Get again - should reuse
        let v = pool.get();
        assert!(v.is_empty()); // cleared
        assert!(v.capacity() >= cap); // capacity preserved
    }

    #[test]
    fn test_vec_pool_max_size() {
        let pool = VecPool::<u32>::new(2);

        // Fill pool
        pool.put(vec![1, 2, 3]);
        pool.put(vec![4, 5, 6]);
        assert_eq!(pool.cached_count(), 2);

        // Third one should be dropped (pool full)
        pool.put(vec![7, 8, 9]);
        assert_eq!(pool.cached_count(), 2);
    }

    #[test]
    fn test_pooled_vec_auto_return() {
        let pool = VecPool::<u32>::new(4);

        {
            let mut pv = PooledVec::new(&pool);
            pv.push(1);
            pv.push(2);
            // Drops here, returns to pool
        }

        assert_eq!(pool.cached_count(), 1);
    }

    #[test]
    fn test_pooled_vec_take() {
        let pool = VecPool::<u32>::new(4);

        let pv = PooledVec::new(&pool);
        let _v = pv.take(); // Takes ownership, does NOT return to pool

        assert_eq!(pool.cached_count(), 0);
    }

    #[test]
    fn test_pool_with_capacity() {
        let pool = VecPool::<u32>::new(4);

        let v = pool.get_with_capacity(100);
        assert!(v.capacity() >= 100);

        pool.put(v);

        // Reuse should preserve capacity
        let v = pool.get();
        assert!(v.capacity() >= 100);
    }
}
