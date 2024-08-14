use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::task::{JoinError, JoinHandle};

pub struct TaskSet<T, U>
where
    T: Unpin,
{
    tasks: Vec<(T, JoinHandle<U>)>,
    next_to_poll: usize,
}

impl<T, U> TaskSet<T, U>
where
    T: Unpin,
{
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_to_poll: 0,
        }
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn add_handle(&mut self, data: T, handle: JoinHandle<U>) {
        self.tasks.push((data, handle));
    }

    pub fn join_next(&mut self) -> JoinNext<'_, T, U> {
        JoinNext {
            tasks: &mut self.tasks,
            next_to_poll: &mut self.next_to_poll,
        }
    }

    pub fn abort_all(&self) {
        () = self
            .tasks
            .iter()
            .for_each(|(_, join_handle)| join_handle.abort());
    }
}

impl<T, U> Default for TaskSet<T, U>
where
    T: Unpin,
{
    fn default() -> Self {
        const { Self::new() }
    }
}

impl<T, U> Drop for TaskSet<T, U>
where
    T: Unpin,
{
    fn drop(&mut self) {
        self.abort_all();
    }
}

#[must_use]
pub struct JoinNext<'r, T, U> {
    tasks: &'r mut Vec<(T, JoinHandle<U>)>,
    next_to_poll: &'r mut usize,
}

impl<'r, T, U> Future for JoinNext<'r, T, U>
where
    T: Unpin,
{
    type Output = Option<(T, Result<U, JoinError>)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        if this.tasks.is_empty() {
            Poll::Ready(None)
        } else {
            let maybe_task_result = (*this.next_to_poll..this.tasks.len())
                .chain(0..*this.next_to_poll)
                .find_map(|index| {
                    let task = Pin::new(&mut this.tasks[index].1).poll(cx);

                    match task {
                        Poll::Ready(result) => {
                            *this.next_to_poll = index;

                            Some((this.tasks.swap_remove(index).0, result))
                        },
                        Poll::Pending => None,
                    }
                });

            match maybe_task_result {
                Some(output) => Poll::Ready(Some(output)),
                None => Poll::Pending,
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn test() {
    use std::{fmt::Debug, marker::Unpin, time::Duration};

    use tokio::{spawn, time::sleep};

    fn print_tasks_data<T, U>(task_set: &TaskSet<T, U>)
    where
        T: Debug + Unpin,
    {
        println!(
            "{:?}",
            task_set
                .tasks
                .iter()
                .map(|(data, _)| data)
                .collect::<Vec<&T>>()
        );
    }

    let mut task_set = TaskSet::new();

    task_set.add_handle(1, spawn(sleep(Duration::from_secs(15))));

    task_set.add_handle(2, spawn(sleep(Duration::from_secs(5))));

    task_set.add_handle(
        3,
        spawn(async {
            sleep(Duration::from_secs(10)).await;

            panic!()
        }),
    );

    print_tasks_data(&task_set);

    assert!(matches!(task_set.join_next().await, Some((2, Ok(())))));

    print_tasks_data(&task_set);

    assert!(matches!(task_set.join_next().await, Some((3, Err(_)))));

    print_tasks_data(&task_set);

    assert!(matches!(task_set.join_next().await, Some((1, Ok(())))));

    print_tasks_data(&task_set);

    assert!(task_set.join_next().await.is_none());
}
