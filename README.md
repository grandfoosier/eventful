# eventful

Design decisions:
EventRecord contains payload_hash
    on insert, if hash mismatch return error
    what if in queue when mismatch? (will return ok)
Used Mutex instead of RwLock
