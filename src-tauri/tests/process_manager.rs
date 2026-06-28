//! Property-Based Test: 单进程约束
//!
//! **Property 12: 单进程约束**
//! 验证运行中尝试 spawn 返回 ProcessAlreadyRunning
//!
//! For any moment if an Agent_Process is in running state,
//! attempting to spawn a new process SHALL return `ProcessAlreadyRunning`
//! error without starting a new process.
//!
//! **Validates: Requirements 11.6**

use app_lib::core::cli_agents::process_manager::{AgentProcessManager, ProcessState};
use app_lib::core::rig_agents::AgentError;
use proptest::prelude::*;

// ─── Generators ─────────────────────────────────────────────────────────────

/// Generate a random PID value (simulating a running process)
fn gen_pid() -> impl Strategy<Value = u32> {
    1..u32::MAX
}

/// Generate random number of concurrent spawn attempts (2-10)
fn gen_spawn_attempts() -> impl Strategy<Value = usize> {
    2..10usize
}



// ─── Property 12: 单进程约束 ────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 11.6**
    ///
    /// Property 12: When a process is running, check_spawn_guard() returns
    /// ProcessAlreadyRunning error. The single-process constraint prevents
    /// spawning a new process while one is already active.
    #[test]
    fn prop_single_process_constraint_rejects_when_running(pid in gen_pid()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let pm = AgentProcessManager::new();

            // Initially should be idle — guard should allow spawn
            let guard_result = pm.check_spawn_guard().await;
            prop_assert!(
                guard_result.is_ok(),
                "Guard should allow spawn when idle, got: {:?}",
                guard_result
            );

            // Simulate a running process
            pm.set_running_for_test(pid).await;

            // Verify state is Running
            let state = pm.status().await;
            prop_assert!(
                matches!(state, ProcessState::Running { .. }),
                "Expected Running state after set_running_for_test, got: {:?}",
                state
            );

            // Now guard should reject spawn
            let guard_result = pm.check_spawn_guard().await;
            prop_assert!(
                matches!(guard_result, Err(AgentError::ProcessAlreadyRunning)),
                "Guard should return ProcessAlreadyRunning when process is active, got: {:?}",
                guard_result
            );

            // Clean up
            pm.clear_for_test().await;
            Ok(())
        })?;
    }

    /// **Validates: Requirements 11.6**
    ///
    /// Property 12 (supplemental): After a process is cleared, the guard allows
    /// spawning again. This verifies the constraint is properly released.
    #[test]
    fn prop_single_process_constraint_allows_after_clear(pid in gen_pid()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let pm = AgentProcessManager::new();

            // Set running
            pm.set_running_for_test(pid).await;

            // Guard rejects
            let result = pm.check_spawn_guard().await;
            prop_assert!(
                matches!(result, Err(AgentError::ProcessAlreadyRunning)),
                "Expected ProcessAlreadyRunning, got: {:?}",
                result
            );

            // Clear the process
            pm.clear_for_test().await;

            // Verify state is back to Idle
            let state = pm.status().await;
            prop_assert!(
                matches!(state, ProcessState::Idle),
                "Expected Idle state after clear, got: {:?}",
                state
            );

            // Guard should now allow spawn
            let result = pm.check_spawn_guard().await;
            prop_assert!(
                result.is_ok(),
                "Guard should allow spawn after clear, got: {:?}",
                result
            );

            Ok(())
        })?;
    }

    /// **Validates: Requirements 11.6**
    ///
    /// Property 12 (supplemental): Multiple consecutive spawn guard checks
    /// while a process is running all return ProcessAlreadyRunning consistently.
    #[test]
    fn prop_single_process_constraint_consistent_rejection(
        pid in gen_pid(),
        attempts in gen_spawn_attempts(),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let pm = AgentProcessManager::new();

            // Set running
            pm.set_running_for_test(pid).await;

            // All N attempts should be rejected
            for i in 0..attempts {
                let result = pm.check_spawn_guard().await;
                prop_assert!(
                    matches!(result, Err(AgentError::ProcessAlreadyRunning)),
                    "Attempt {} of {} should return ProcessAlreadyRunning, got: {:?}",
                    i + 1,
                    attempts,
                    result
                );
            }

            // Clean up
            pm.clear_for_test().await;
            Ok(())
        })?;
    }

    /// **Validates: Requirements 11.6**
    ///
    /// Property 12 (supplemental): New AgentProcessManager always starts idle,
    /// meaning the guard allows spawn on a fresh instance regardless of PID value.
    #[test]
    fn prop_new_manager_starts_idle(_pid in gen_pid()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let pm = AgentProcessManager::new();

            // Fresh manager should be idle
            let state = pm.status().await;
            prop_assert!(
                matches!(state, ProcessState::Idle),
                "New manager should be Idle, got: {:?}",
                state
            );

            // Guard should allow spawn
            let result = pm.check_spawn_guard().await;
            prop_assert!(
                result.is_ok(),
                "New manager guard should allow spawn, got: {:?}",
                result
            );

            Ok(())
        })?;
    }
}
