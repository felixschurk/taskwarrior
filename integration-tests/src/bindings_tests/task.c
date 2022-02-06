#include <stdlib.h>
#include <string.h>
#include "unity.h"
#include "taskchampion.h"

// creating a task succeeds and the resulting task looks good
static void test_task_creation(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    TEST_ASSERT_EQUAL(TC_STATUS_PENDING, tc_task_get_status(task));

    TCString *desc = tc_task_get_description(task);
    TEST_ASSERT_NOT_NULL(desc);
    TEST_ASSERT_EQUAL_STRING("my task", tc_string_content(desc));
    tc_string_free(desc);

    tc_task_free(task);

    tc_replica_free(rep);
}

// freeing a mutable task works, marking it immutable
static void test_task_free_mutable_task(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    TEST_ASSERT_EQUAL(TC_STATUS_PENDING, tc_task_get_status(task));
    TCUuid uuid = tc_task_get_uuid(task);

    tc_task_to_mut(task, rep);
    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_status(task, TC_STATUS_DELETED));
    TEST_ASSERT_EQUAL(TC_STATUS_DELETED, tc_task_get_status(task));

    tc_task_free(task); // implicitly converts to immut

    task = tc_replica_get_task(rep, uuid);
    TEST_ASSERT_NOT_NULL(task);
    TEST_ASSERT_EQUAL(TC_STATUS_DELETED, tc_task_get_status(task));
    tc_task_free(task);

    tc_replica_free(rep);
}

// updating status on a task works
static void test_task_get_set_status(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    TEST_ASSERT_EQUAL(TC_STATUS_PENDING, tc_task_get_status(task));

    tc_task_to_mut(task, rep);
    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_status(task, TC_STATUS_DELETED));
    TEST_ASSERT_EQUAL(TC_STATUS_DELETED, tc_task_get_status(task)); // while mut
    tc_task_to_immut(task);
    TEST_ASSERT_EQUAL(TC_STATUS_DELETED, tc_task_get_status(task)); // while immut

    tc_task_free(task);

    tc_replica_free(rep);
}

// updating description on a task works
static void test_task_get_set_description(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    TCString *desc;

    tc_task_to_mut(task, rep);
    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_description(task, tc_string_borrow("updated")));

    TEST_ASSERT_TRUE(desc = tc_task_get_description(task));
    TEST_ASSERT_NOT_NULL(desc);
    TEST_ASSERT_EQUAL_STRING("updated", tc_string_content(desc));
    tc_string_free(desc);

    tc_task_to_immut(task);

    desc = tc_task_get_description(task);
    TEST_ASSERT_NOT_NULL(desc);
    TEST_ASSERT_EQUAL_STRING("updated", tc_string_content(desc));
    tc_string_free(desc);

    tc_task_free(task);

    tc_replica_free(rep);
}

// updating entry on a task works
static void test_task_get_set_entry(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    // creation of a task sets entry to current time
    TEST_ASSERT_NOT_EQUAL(0, tc_task_get_entry(task));

    tc_task_to_mut(task, rep);

    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_entry(task, 1643679997));
    TEST_ASSERT_EQUAL(1643679997, tc_task_get_entry(task));

    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_entry(task, 0));
    TEST_ASSERT_EQUAL(0, tc_task_get_entry(task));

    tc_task_free(task);

    tc_replica_free(rep);
}

// updating wait on a task works
static void test_task_get_set_wait_and_is_waiting(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    // wait is not set on creation
    TEST_ASSERT_EQUAL(0, tc_task_get_wait(task));
    TEST_ASSERT_FALSE(tc_task_is_waiting(task));

    tc_task_to_mut(task, rep);

    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_wait(task, 3643679997)); // 2085
    TEST_ASSERT_EQUAL(3643679997, tc_task_get_wait(task));
    TEST_ASSERT_TRUE(tc_task_is_waiting(task));

    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_wait(task, 643679997)); // THE PAST!
    TEST_ASSERT_EQUAL(643679997, tc_task_get_wait(task));
    TEST_ASSERT_FALSE(tc_task_is_waiting(task));

    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_wait(task, 0));
    TEST_ASSERT_EQUAL(0, tc_task_get_wait(task));
    TEST_ASSERT_FALSE(tc_task_is_waiting(task));

    tc_task_free(task);

    tc_replica_free(rep);
}

// updating modified on a task works
static void test_task_get_set_modified(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    // creation of a task sets modified to current time
    TEST_ASSERT_NOT_EQUAL(0, tc_task_get_modified(task));

    tc_task_to_mut(task, rep);

    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_set_modified(task, 1643679997));
    TEST_ASSERT_EQUAL(1643679997, tc_task_get_modified(task));

    // zero is not allowed
    TEST_ASSERT_EQUAL(TC_RESULT_ERROR, tc_task_set_modified(task, 0));

    tc_task_free(task);

    tc_replica_free(rep);
}

// starting and stopping a task works, as seen by tc_task_is_active
static void test_task_start_stop_is_active(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    TEST_ASSERT_FALSE(tc_task_is_active(task));

    tc_task_to_mut(task, rep);

    TEST_ASSERT_FALSE(tc_task_is_active(task));
    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_start(task));
    TEST_ASSERT_TRUE(tc_task_is_active(task));
    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_stop(task));
    TEST_ASSERT_FALSE(tc_task_is_active(task));

    tc_task_free(task);
    tc_replica_free(rep);
}

// tc_task_done and delete work and set the status
static void test_task_done_and_delete(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    tc_task_to_mut(task, rep);

    TEST_ASSERT_EQUAL(TC_STATUS_PENDING, tc_task_get_status(task));
    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_done(task));
    TEST_ASSERT_EQUAL(TC_STATUS_COMPLETED, tc_task_get_status(task));
    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_delete(task));
    TEST_ASSERT_EQUAL(TC_STATUS_DELETED, tc_task_get_status(task));

    tc_task_free(task);
    tc_replica_free(rep);
}

// adding and removing tags to a task works, and invalid tags are rejected
static void test_task_add_remove_has_tag(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    tc_task_to_mut(task, rep);

    TEST_ASSERT_FALSE(tc_task_has_tag(task, tc_string_borrow("next")));

    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_add_tag(task, tc_string_borrow("next")));
    TEST_ASSERT_NULL(tc_task_error(task));

    TEST_ASSERT_TRUE(tc_task_has_tag(task, tc_string_borrow("next")));

    // invalid - synthetic tag
    TEST_ASSERT_EQUAL(TC_RESULT_ERROR, tc_task_add_tag(task, tc_string_borrow("PENDING")));
    TCString *err = tc_task_error(task);
    TEST_ASSERT_NOT_NULL(err);
    tc_string_free(err);

    // invald - not a valid tag string
    TEST_ASSERT_EQUAL(TC_RESULT_ERROR, tc_task_add_tag(task, tc_string_borrow("my tag")));
    err = tc_task_error(task);
    TEST_ASSERT_NOT_NULL(err);
    tc_string_free(err);

    // invald - not utf-8
    TEST_ASSERT_EQUAL(TC_RESULT_ERROR, tc_task_add_tag(task, tc_string_borrow("\xf0\x28\x8c\x28")));
    err = tc_task_error(task);
    TEST_ASSERT_NOT_NULL(err);
    tc_string_free(err);

    TEST_ASSERT_TRUE(tc_task_has_tag(task, tc_string_borrow("next")));

    // remove the tag
    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_remove_tag(task, tc_string_borrow("next")));
    TEST_ASSERT_NULL(tc_task_error(task));

    TEST_ASSERT_FALSE(tc_task_has_tag(task, tc_string_borrow("next")));

    tc_task_free(task);
    tc_replica_free(rep);
}

// get_tags returns the list of tags
static void test_task_get_tags(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCTask *task = tc_replica_new_task(
            rep,
            TC_STATUS_PENDING,
            tc_string_borrow("my task"));
    TEST_ASSERT_NOT_NULL(task);

    tc_task_to_mut(task, rep);

    TEST_ASSERT_EQUAL(TC_RESULT_OK, tc_task_add_tag(task, tc_string_borrow("next")));

    TCTags tags = tc_task_get_tags(task);

    int found_pending = false, found_next = false;
    for (size_t i = 0; i < tags.len; i++) {
        if (strcmp("PENDING", tc_string_content(tags.items[i])) == 0) {
            found_pending = true;
        }
        if (strcmp("next", tc_string_content(tags.items[i])) == 0) {
            found_next = true;
        }
    }
    TEST_ASSERT_TRUE(found_pending);
    TEST_ASSERT_TRUE(found_next);

    tc_tags_free(&tags);
    TEST_ASSERT_NULL(tags.items);

    tc_task_free(task);
    tc_replica_free(rep);
}

int task_tests(void) {
    UNITY_BEGIN();
    // each test case above should be named here, in order.
    RUN_TEST(test_task_creation);
    RUN_TEST(test_task_free_mutable_task);
    RUN_TEST(test_task_get_set_status);
    RUN_TEST(test_task_get_set_description);
    RUN_TEST(test_task_get_set_entry);
    RUN_TEST(test_task_get_set_modified);
    RUN_TEST(test_task_get_set_wait_and_is_waiting);
    RUN_TEST(test_task_start_stop_is_active);
    RUN_TEST(test_task_done_and_delete);
    RUN_TEST(test_task_add_remove_has_tag);
    RUN_TEST(test_task_get_tags);
    return UNITY_END();
}
