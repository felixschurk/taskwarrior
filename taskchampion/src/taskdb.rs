use crate::errors::Error;
use crate::server::{Server, VersionAdd};
use crate::taskstorage::{Operation, TaskMap, TaskStorage, TaskStorageTxn};
use failure::Fallible;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str;
use uuid::Uuid;

pub struct TaskDB {
    storage: Box<dyn TaskStorage>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Version {
    version: u64,
    operations: Vec<Operation>,
}

impl TaskDB {
    /// Create a new TaskDB with the given backend storage
    pub fn new(storage: Box<dyn TaskStorage>) -> TaskDB {
        TaskDB { storage }
    }

    #[cfg(test)]
    pub fn new_inmemory() -> TaskDB {
        TaskDB::new(Box::new(crate::taskstorage::InMemoryStorage::new()))
    }

    /// Apply an operation to the TaskDB.  Aside from synchronization operations, this is the only way
    /// to modify the TaskDB.  In cases where an operation does not make sense, this function will do
    /// nothing and return an error (but leave the TaskDB in a consistent state).
    pub fn apply(&mut self, op: Operation) -> Fallible<()> {
        // TODO: differentiate error types here?
        let mut txn = self.storage.txn()?;
        if let err @ Err(_) = TaskDB::apply_op(txn.as_mut(), &op) {
            return err;
        }
        txn.add_operation(op)?;
        txn.commit()?;
        Ok(())
    }

    fn apply_op(txn: &mut dyn TaskStorageTxn, op: &Operation) -> Fallible<()> {
        match op {
            &Operation::Create { uuid } => {
                // insert if the task does not already exist
                if !txn.create_task(uuid)? {
                    return Err(Error::DBError(format!("Task {} already exists", uuid)).into());
                }
            }
            &Operation::Delete { ref uuid } => {
                if !txn.delete_task(uuid)? {
                    return Err(Error::DBError(format!("Task {} does not exist", uuid)).into());
                }
            }
            &Operation::Update {
                ref uuid,
                ref property,
                ref value,
                timestamp: _,
            } => {
                // update if this task exists, otherwise ignore
                if let Some(task) = txn.get_task(uuid)? {
                    let mut task = task.clone();
                    // TODO: update working_set if this is changing state to or from pending
                    match value {
                        Some(ref val) => task.insert(property.to_string(), val.clone()),
                        None => task.remove(property),
                    };
                    txn.set_task(uuid.clone(), task)?;
                } else {
                    return Err(Error::DBError(format!("Task {} does not exist", uuid)).into());
                }
            }
        }

        Ok(())
    }

    /// Get all tasks.
    pub fn all_tasks<'a>(&'a mut self) -> Fallible<Vec<(Uuid, TaskMap)>> {
        let mut txn = self.storage.txn()?;
        txn.all_tasks()
    }

    /// Get the UUIDs of all tasks
    pub fn all_task_uuids<'a>(&'a mut self) -> Fallible<Vec<Uuid>> {
        let mut txn = self.storage.txn()?;
        txn.all_task_uuids()
    }

    /// Get the working set
    pub fn working_set<'a>(&'a mut self) -> Fallible<Vec<Option<Uuid>>> {
        let mut txn = self.storage.txn()?;
        txn.get_working_set()
    }

    /// Get a single task, by uuid.
    pub fn get_task(&mut self, uuid: &Uuid) -> Fallible<Option<TaskMap>> {
        let mut txn = self.storage.txn()?;
        txn.get_task(uuid)
    }

    /// Rebuild the working set using a function to identify tasks that should be in the set.  This
    /// renumbers the existing working-set tasks to eliminate gaps, and also adds any tasks that
    /// are not already in the working set but should be.  The rebuild occurs in a single
    /// trasnsaction against the storage backend.
    pub fn rebuild_working_set<F>(&mut self, in_working_set: F) -> Fallible<()>
    where
        F: Fn(&TaskMap) -> bool,
    {
        let mut txn = self.storage.txn()?;

        let mut new_ws = vec![];
        let mut seen = HashSet::new();

        // The goal here is for existing working-set items to be "compressed' down to index 1, so
        // we begin by scanning the current working set and inserting any tasks that should still
        // be in the set into new_ws, implicitly dropping any tasks that are no longer in the
        // working set.
        for elt in txn.get_working_set()? {
            if let Some(uuid) = elt {
                if let Some(task) = txn.get_task(&uuid)? {
                    if in_working_set(&task) {
                        new_ws.push(uuid.clone());
                        seen.insert(uuid);
                    }
                }
            }
        }

        // Now go hunting for tasks that should be in this list but are not, adding them at the
        // end of the list.
        for (uuid, task) in txn.all_tasks()? {
            if !seen.contains(&uuid) {
                if in_working_set(&task) {
                    new_ws.push(uuid.clone());
                }
            }
        }

        // clear and re-write the entire working set, in order
        txn.clear_working_set()?;
        for uuid in new_ws.drain(0..new_ws.len()) {
            txn.add_to_working_set(&uuid)?;
        }

        txn.commit()?;
        Ok(())
    }

    /// Add the given uuid to the working set and return its index; if it is already in the working
    /// set, its index is returned.  This does *not* renumber any existing tasks.
    pub fn add_to_working_set(&mut self, uuid: &Uuid) -> Fallible<u64> {
        let mut txn = self.storage.txn()?;
        // search for an existing entry for this task..
        for (i, elt) in txn.get_working_set()?.iter().enumerate() {
            if *elt == Some(*uuid) {
                // (note that this drops the transaction with no changes made)
                return Ok(i as u64);
            }
        }
        // and if not found, add one
        let i = txn.add_to_working_set(uuid)?;
        txn.commit()?;
        Ok(i)
    }

    /// Sync to the given server, pulling remote changes and pushing local changes.
    pub fn sync(&mut self, username: &str, server: &mut dyn Server) -> Fallible<()> {
        let mut txn = self.storage.txn()?;

        // retry synchronizing until the server accepts our version (this allows for races between
        // replicas trying to sync to the same server)
        loop {
            // first pull changes and "rebase" on top of them
            let new_versions = server.get_versions(username, txn.base_version()?);
            for version_blob in new_versions {
                let version_str = str::from_utf8(&version_blob).unwrap();
                let version: Version = serde_json::from_str(version_str).unwrap();
                assert_eq!(version.version, txn.base_version()? + 1);
                println!("applying version {:?} from server", version.version);

                TaskDB::apply_version(txn.as_mut(), version)?;
            }

            let operations: Vec<Operation> = txn.operations()?.iter().map(|o| o.clone()).collect();
            if operations.len() == 0 {
                // nothing to sync back to the server..
                break;
            }

            // now make a version of our local changes and push those
            let new_version = Version {
                version: txn.base_version()? + 1,
                operations: operations,
            };
            let new_version_str = serde_json::to_string(&new_version).unwrap();
            println!("sending version {:?} to server", new_version.version);
            if let VersionAdd::Ok =
                server.add_version(username, new_version.version, new_version_str.into())
            {
                txn.set_base_version(new_version.version)?;
                txn.set_operations(vec![])?;
                break;
            }
        }

        txn.commit()?;
        Ok(())
    }

    fn apply_version(txn: &mut dyn TaskStorageTxn, mut version: Version) -> Fallible<()> {
        // The situation here is that the server has already applied all server operations, and we
        // have already applied all local operations, so states have diverged by several
        // operations.  We need to figure out what operations to apply locally and on the server in
        // order to return to the same state.
        //
        // Operational transforms provide this on an operation-by-operation basis.  To break this
        // down, we treat each server operation individually, in order.  For each such operation,
        // we start in this state:
        //
        //
        //      base state-*
        //                / \-server op
        //               *   *
        //     local    / \ /
        //     ops     *   *
        //            / \ / new
        //           *   * local
        //   local  / \ / ops
        //   state-*   *
        //      new-\ /
        // server op *-new local state
        //
        // This is slightly complicated by the fact that the transform function can return None,
        // indicating no operation is required.  If this happens for a local op, we can just omit
        // it.  If it happens for server op, then we must copy the remaining local ops.
        let mut local_operations: Vec<Operation> = txn.operations()?;
        for server_op in version.operations.drain(..) {
            let mut new_local_ops = Vec::with_capacity(local_operations.len());
            let mut svr_op = Some(server_op);
            for local_op in local_operations.drain(..) {
                if let Some(o) = svr_op {
                    let (new_server_op, new_local_op) = Operation::transform(o, local_op);
                    svr_op = new_server_op;
                    if let Some(o) = new_local_op {
                        new_local_ops.push(o);
                    }
                } else {
                    new_local_ops.push(local_op);
                }
            }
            if let Some(o) = svr_op {
                if let Err(e) = TaskDB::apply_op(txn, &o) {
                    println!("Invalid operation when syncing: {} (ignored)", e);
                }
            }
            local_operations = new_local_ops;
        }
        txn.set_base_version(version.version)?;
        txn.set_operations(local_operations)?;
        Ok(())
    }

    // functions for supporting tests

    #[cfg(test)]
    pub(crate) fn sorted_tasks(&mut self) -> Vec<(Uuid, Vec<(String, String)>)> {
        let mut res: Vec<(Uuid, Vec<(String, String)>)> = self
            .all_tasks()
            .unwrap()
            .iter()
            .map(|(u, t)| {
                let mut t = t
                    .iter()
                    .map(|(p, v)| (p.clone(), v.clone()))
                    .collect::<Vec<(String, String)>>();
                t.sort();
                (u.clone(), t)
            })
            .collect();
        res.sort();
        res
    }

    #[cfg(test)]
    pub(crate) fn operations(&mut self) -> Vec<Operation> {
        let mut txn = self.storage.txn().unwrap();
        txn.operations()
            .unwrap()
            .iter()
            .map(|o| o.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskstorage::InMemoryStorage;
    use crate::testing::testserver::TestServer;
    use chrono::Utc;
    use proptest::prelude::*;
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn test_apply_create() {
        let mut db = TaskDB::new_inmemory();
        let uuid = Uuid::new_v4();
        let op = Operation::Create { uuid };
        db.apply(op.clone()).unwrap();

        assert_eq!(db.sorted_tasks(), vec![(uuid, vec![]),]);
        assert_eq!(db.operations(), vec![op]);
    }

    #[test]
    fn test_apply_create_exists() {
        let mut db = TaskDB::new_inmemory();
        let uuid = Uuid::new_v4();
        let op = Operation::Create { uuid };
        db.apply(op.clone()).unwrap();
        assert_eq!(
            db.apply(op.clone()).err().unwrap().to_string(),
            format!("Task Database Error: Task {} already exists", uuid)
        );

        assert_eq!(db.sorted_tasks(), vec![(uuid, vec![])]);
        assert_eq!(db.operations(), vec![op]);
    }

    #[test]
    fn test_apply_create_update() {
        let mut db = TaskDB::new_inmemory();
        let uuid = Uuid::new_v4();
        let op1 = Operation::Create { uuid };
        db.apply(op1.clone()).unwrap();
        let op2 = Operation::Update {
            uuid,
            property: String::from("title"),
            value: Some("my task".into()),
            timestamp: Utc::now(),
        };
        db.apply(op2.clone()).unwrap();

        assert_eq!(
            db.sorted_tasks(),
            vec![(uuid, vec![("title".into(), "my task".into())])]
        );
        assert_eq!(db.operations(), vec![op1, op2]);
    }

    #[test]
    fn test_apply_create_update_delete_prop() {
        let mut db = TaskDB::new_inmemory();
        let uuid = Uuid::new_v4();
        let op1 = Operation::Create { uuid };
        db.apply(op1.clone()).unwrap();

        let op2 = Operation::Update {
            uuid,
            property: String::from("title"),
            value: Some("my task".into()),
            timestamp: Utc::now(),
        };
        db.apply(op2.clone()).unwrap();

        let op3 = Operation::Update {
            uuid,
            property: String::from("priority"),
            value: Some("H".into()),
            timestamp: Utc::now(),
        };
        db.apply(op3.clone()).unwrap();

        let op4 = Operation::Update {
            uuid,
            property: String::from("title"),
            value: None,
            timestamp: Utc::now(),
        };
        db.apply(op4.clone()).unwrap();

        let mut exp = HashMap::new();
        let mut task = HashMap::new();
        task.insert(String::from("priority"), String::from("H"));
        exp.insert(uuid, task);
        assert_eq!(
            db.sorted_tasks(),
            vec![(uuid, vec![("priority".into(), "H".into())])]
        );
        assert_eq!(db.operations(), vec![op1, op2, op3, op4]);
    }

    #[test]
    fn test_apply_update_does_not_exist() {
        let mut db = TaskDB::new_inmemory();
        let uuid = Uuid::new_v4();
        let op = Operation::Update {
            uuid,
            property: String::from("title"),
            value: Some("my task".into()),
            timestamp: Utc::now(),
        };
        assert_eq!(
            db.apply(op).err().unwrap().to_string(),
            format!("Task Database Error: Task {} does not exist", uuid)
        );

        assert_eq!(db.sorted_tasks(), vec![]);
        assert_eq!(db.operations(), vec![]);
    }

    #[test]
    fn test_apply_create_delete() {
        let mut db = TaskDB::new_inmemory();
        let uuid = Uuid::new_v4();
        let op1 = Operation::Create { uuid };
        db.apply(op1.clone()).unwrap();

        let op2 = Operation::Delete { uuid };
        db.apply(op2.clone()).unwrap();

        assert_eq!(db.sorted_tasks(), vec![]);
        assert_eq!(db.operations(), vec![op1, op2]);
    }

    #[test]
    fn test_apply_delete_not_present() {
        let mut db = TaskDB::new_inmemory();
        let uuid = Uuid::new_v4();

        let op1 = Operation::Delete { uuid };
        assert_eq!(
            db.apply(op1).err().unwrap().to_string(),
            format!("Task Database Error: Task {} does not exist", uuid)
        );

        assert_eq!(db.sorted_tasks(), vec![]);
        assert_eq!(db.operations(), vec![]);
    }

    #[test]
    fn rebuild_working_set() -> Fallible<()> {
        let mut db = TaskDB::new_inmemory();
        let uuids = vec![
            Uuid::new_v4(), // 0: pending, not already in working set
            Uuid::new_v4(), // 1: pending, already in working set
            Uuid::new_v4(), // 2: not pending, not already in working set
            Uuid::new_v4(), // 3: not pending, already in working set
            Uuid::new_v4(), // 4: pending, already in working set
        ];

        // add everything to the TaskDB
        for uuid in &uuids {
            db.apply(Operation::Create { uuid: uuid.clone() })?;
        }
        for i in &[0usize, 1, 4] {
            db.apply(Operation::Update {
                uuid: uuids[*i].clone(),
                property: String::from("status"),
                value: Some("pending".into()),
                timestamp: Utc::now(),
            })?;
        }

        // set the existing working_set as we want it
        {
            let mut txn = db.storage.txn()?;
            txn.clear_working_set()?;

            for i in &[1usize, 3, 4] {
                txn.add_to_working_set(&uuids[*i])?;
            }

            txn.commit()?;
        }

        assert_eq!(
            db.working_set()?,
            vec![
                None,
                Some(uuids[1].clone()),
                Some(uuids[3].clone()),
                Some(uuids[4].clone())
            ]
        );

        db.rebuild_working_set(|t| {
            if let Some(status) = t.get("status") {
                status == "pending"
            } else {
                false
            }
        })?;

        // uuids[1] and uuids[4] are already in the working set, so are compressed
        // to the top, and then uuids[0] is added.
        assert_eq!(
            db.working_set()?,
            vec![
                None,
                Some(uuids[1].clone()),
                Some(uuids[4].clone()),
                Some(uuids[0].clone())
            ]
        );

        Ok(())
    }

    fn newdb() -> TaskDB {
        TaskDB::new(Box::new(InMemoryStorage::new()))
    }

    #[test]
    fn test_sync() {
        let mut server = TestServer::new();

        let mut db1 = newdb();
        db1.sync("me", &mut server).unwrap();

        let mut db2 = newdb();
        db2.sync("me", &mut server).unwrap();

        // make some changes in parallel to db1 and db2..
        let uuid1 = Uuid::new_v4();
        db1.apply(Operation::Create { uuid: uuid1 }).unwrap();
        db1.apply(Operation::Update {
            uuid: uuid1,
            property: "title".into(),
            value: Some("my first task".into()),
            timestamp: Utc::now(),
        })
        .unwrap();

        let uuid2 = Uuid::new_v4();
        db2.apply(Operation::Create { uuid: uuid2 }).unwrap();
        db2.apply(Operation::Update {
            uuid: uuid2,
            property: "title".into(),
            value: Some("my second task".into()),
            timestamp: Utc::now(),
        })
        .unwrap();

        // and synchronize those around
        db1.sync("me", &mut server).unwrap();
        db2.sync("me", &mut server).unwrap();
        db1.sync("me", &mut server).unwrap();
        assert_eq!(db1.sorted_tasks(), db2.sorted_tasks());

        // now make updates to the same task on both sides
        db1.apply(Operation::Update {
            uuid: uuid2,
            property: "priority".into(),
            value: Some("H".into()),
            timestamp: Utc::now(),
        })
        .unwrap();
        db2.apply(Operation::Update {
            uuid: uuid2,
            property: "project".into(),
            value: Some("personal".into()),
            timestamp: Utc::now(),
        })
        .unwrap();

        // and synchronize those around
        db1.sync("me", &mut server).unwrap();
        db2.sync("me", &mut server).unwrap();
        db1.sync("me", &mut server).unwrap();
        assert_eq!(db1.sorted_tasks(), db2.sorted_tasks());
    }

    #[test]
    fn test_sync_create_delete() {
        let mut server = TestServer::new();

        let mut db1 = newdb();
        db1.sync("me", &mut server).unwrap();

        let mut db2 = newdb();
        db2.sync("me", &mut server).unwrap();

        // create and update a task..
        let uuid = Uuid::new_v4();
        db1.apply(Operation::Create { uuid }).unwrap();
        db1.apply(Operation::Update {
            uuid: uuid,
            property: "title".into(),
            value: Some("my first task".into()),
            timestamp: Utc::now(),
        })
        .unwrap();

        // and synchronize those around
        db1.sync("me", &mut server).unwrap();
        db2.sync("me", &mut server).unwrap();
        db1.sync("me", &mut server).unwrap();
        assert_eq!(db1.sorted_tasks(), db2.sorted_tasks());

        // delete and re-create the task on db1
        db1.apply(Operation::Delete { uuid }).unwrap();
        db1.apply(Operation::Create { uuid }).unwrap();
        db1.apply(Operation::Update {
            uuid: uuid,
            property: "title".into(),
            value: Some("my second task".into()),
            timestamp: Utc::now(),
        })
        .unwrap();

        // and on db2, update a property of the task
        db2.apply(Operation::Update {
            uuid: uuid,
            property: "project".into(),
            value: Some("personal".into()),
            timestamp: Utc::now(),
        })
        .unwrap();

        db1.sync("me", &mut server).unwrap();
        db2.sync("me", &mut server).unwrap();
        db1.sync("me", &mut server).unwrap();
        assert_eq!(db1.sorted_tasks(), db2.sorted_tasks());
    }

    #[derive(Debug)]
    enum Action {
        Op(Operation),
        Sync,
    }

    fn action_sequence_strategy() -> impl Strategy<Value = Vec<(Action, u8)>> {
        // Create, Update, Delete, or Sync on client 1, 2, .., followed by a round of syncs
        "([CUDS][123])*S1S2S3S1S2".prop_map(|seq| {
            let uuid = Uuid::parse_str("83a2f9ef-f455-4195-b92e-a54c161eebfc").unwrap();
            seq.as_bytes()
                .chunks(2)
                .map(|action_on| {
                    let action = match action_on[0] {
                        b'C' => Action::Op(Operation::Create { uuid }),
                        b'U' => Action::Op(Operation::Update {
                            uuid,
                            property: "title".into(),
                            value: Some("foo".into()),
                            timestamp: Utc::now(),
                        }),
                        b'D' => Action::Op(Operation::Delete { uuid }),
                        b'S' => Action::Sync,
                        _ => unreachable!(),
                    };
                    let acton = action_on[1] - b'1';
                    (action, acton)
                })
                .collect::<Vec<(Action, u8)>>()
        })
    }

    proptest! {
        #[test]
        // check that various sequences of operations on mulitple db's do not get the db's into an
        // incompatible state.  The main concern here is that there might be a sequence of create
        // and delete operations that results in a task existing in one TaskDB but not existing in
        // another.  So, the generated sequences focus on a single task UUID.
        fn transform_sequences_of_operations(action_sequence in action_sequence_strategy()) {
            let mut server = TestServer::new();
            let mut dbs = [newdb(), newdb(), newdb()];

            for (action, db) in action_sequence {
                println!("{:?} on db {}", action, db);

                let db = &mut dbs[db as usize];
                match action {
                    Action::Op(op) => {
                        if let Err(e) = db.apply(op) {
                            println!("  {:?} (ignored)", e);
                        }
                    },
                    Action::Sync => db.sync("me", &mut server).unwrap(),
                }
            }

            assert_eq!(dbs[0].sorted_tasks(), dbs[0].sorted_tasks());
            assert_eq!(dbs[1].sorted_tasks(), dbs[2].sorted_tasks());
        }
    }
}
