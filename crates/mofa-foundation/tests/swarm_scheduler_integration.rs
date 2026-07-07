//! swarm scheduler integration tests

use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use petgraph::graph::NodeIndex;

use mofa_foundation::swarm::{
    CoordinationPattern, FailurePolicy, ParallelScheduler, SubtaskDAG, SubtaskExecutorFn,
    SubtaskStatus, SwarmScheduler, SwarmSchedulerConfig, SwarmSubtask,
};
use mofa_kernel::agent::types::error::GlobalError;

fn ok_executor() -> SubtaskExecutorFn {
    Arc::new(
        |_idx: NodeIndex, task: SwarmSubtask| -> BoxFuture<'static, _> {
            Box::pin(async move { Ok(format!("ok:{}", task.id)) })
        },
    )
}

#[tokio::test]
async fn sequential_scheduler_executes_via_pattern_and_marks_completed() {
    let mut dag = SubtaskDAG::new("seq");
    let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));
    let idx_b = dag.add_task(SwarmSubtask::new("B", "Task B"));
    dag.add_dependency(idx_a, idx_b).unwrap();

    let scheduler = CoordinationPattern::Sequential.into_scheduler()
        .expect("Sequential scheduler should be implemented");
    let _summary = scheduler.execute(&mut dag, ok_executor()).await.unwrap();

    assert_eq!(
        dag.get_task(idx_a).unwrap().status,
        SubtaskStatus::Completed
    );
    assert_eq!(
        dag.get_task(idx_b).unwrap().status,
        SubtaskStatus::Completed
    );
}

#[tokio::test]
async fn parallel_scheduler_executes_via_pattern_and_respects_dependencies() {
    let mut dag = SubtaskDAG::new("par");
    let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));
    let idx_b = dag.add_task(SwarmSubtask::new("B", "Task B"));
    let idx_c = dag.add_task(SwarmSubtask::new("C", "Task C"));
    let idx_d = dag.add_task(SwarmSubtask::new("D", "Task D"));

    dag.add_dependency(idx_a, idx_b).unwrap();
    dag.add_dependency(idx_a, idx_c).unwrap();
    dag.add_dependency(idx_b, idx_d).unwrap();
    dag.add_dependency(idx_c, idx_d).unwrap();

    let exec: SubtaskExecutorFn = Arc::new(|_idx: NodeIndex, task: SwarmSubtask| {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(format!("ok:{}", task.id))
        })
    });

    let scheduler = CoordinationPattern::Parallel.into_scheduler()
        .expect("Parallel scheduler should be implemented");
    let _summary = scheduler.execute(&mut dag, exec).await.unwrap();

    for (idx, task) in dag.all_tasks() {
        assert_eq!(
            task.status,
            SubtaskStatus::Completed,
            "expected task {:?} ({}) to be completed",
            idx,
            task.id
        );
    }
}

#[tokio::test]
async fn parallel_fail_fast_cascades_skip() {
    let mut dag = SubtaskDAG::new("par-fail-fast");
    let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));
    let idx_b = dag.add_task(SwarmSubtask::new("B", "Task B"));
    let idx_c = dag.add_task(SwarmSubtask::new("C", "Task C"));

    dag.add_dependency(idx_a, idx_b).unwrap();
    dag.add_dependency(idx_a, idx_c).unwrap();

    let exec: SubtaskExecutorFn = Arc::new(|_idx: NodeIndex, task: SwarmSubtask| {
        Box::pin(async move {
            if task.id == "A" {
                Err(GlobalError::runtime("A failed"))
            } else {
                Ok(format!("ok:{}", task.id))
            }
        })
    });

    let mut config = SwarmSchedulerConfig::default();
    config.failure_policy = FailurePolicy::FailFastCascade;
    let scheduler = ParallelScheduler::with_config(config);
    let _summary = scheduler.execute(&mut dag, exec).await.unwrap();

    assert!(matches!(
        dag.get_task(idx_a).unwrap().status,
        SubtaskStatus::Failed(_)
    ));
    assert_eq!(dag.get_task(idx_b).unwrap().status, SubtaskStatus::Skipped);
    assert_eq!(dag.get_task(idx_c).unwrap().status, SubtaskStatus::Skipped);
}
