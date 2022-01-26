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

// importing a task succeeds and the resulting task looks good
static void test_task_import(void) {
    TCReplica *rep = tc_replica_new_in_memory();
    TEST_ASSERT_NULL(tc_replica_error(rep));

    TCUuid uuid;
    TEST_ASSERT_TRUE(tc_uuid_from_str(tc_string_borrow("23cb25e0-5d1a-4932-8131-594ac6d3a843"), &uuid));
    TCTask *task = tc_replica_import_task_with_uuid(rep, uuid);
    TEST_ASSERT_NOT_NULL(task);

    TEST_ASSERT_EQUAL(TC_STATUS_PENDING, tc_task_get_status(task));

    TCString *desc = tc_task_get_description(task);
    TEST_ASSERT_NOT_NULL(desc);
    TEST_ASSERT_EQUAL_STRING("", tc_string_content(desc)); // default value
    tc_string_free(desc);

    tc_task_free(task);

    tc_replica_free(rep);
}

int task_tests(void) {
    UNITY_BEGIN();
    // each test case above should be named here, in order.
    RUN_TEST(test_task_creation);
    RUN_TEST(test_task_import);
    return UNITY_END();
}
