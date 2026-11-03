#include "config.h"

#include "common/common.h"
#include "common/msg.h"
#include "options/m_config_frontend.h"

#include "user_filters.h"

static bool get_desc_from(const struct mp_user_filter_entry **list, int num,
                          struct m_obj_desc *dst, int index)
{
    if (index >= num)
        return false;
    const struct mp_user_filter_entry *entry = list[index];
    *dst = entry->desc;
    dst->p = entry;
    return true;
}

// --af option

const struct mp_user_filter_entry *af_list[] = {
    &af_scaletempo,
    &af_scaletempo2,
    &af_format,
#if HAVE_RUBBERBAND
    &af_rubberband,
#endif
    &af_drop,
};

static bool get_af_desc(struct m_obj_desc *dst, int index)
{
    return get_desc_from(af_list, MP_ARRAY_SIZE(af_list), dst, index);
}

const struct m_obj_list af_obj_list = {
    .get_desc = get_af_desc,
    .description = "audio filters",
    .allow_disable_entries = true,
};

// --vf option

const struct mp_user_filter_entry *vf_list[] = {
    &vf_format,
    &vf_sub,
#if HAVE_ZIMG
    &vf_fingerprint,
#endif
};

static bool get_vf_desc(struct m_obj_desc *dst, int index)
{
    return get_desc_from(vf_list, MP_ARRAY_SIZE(vf_list), dst, index);
}

const struct m_obj_list vf_obj_list = {
    .get_desc = get_vf_desc,
    .description = "video filters",
    .allow_disable_entries = true,
};

// Create a bidir, single-media filter from command line arguments.
struct mp_filter *mp_create_user_filter(struct mp_filter *parent,
                                        enum mp_output_chain_type type,
                                        const char *name, char **args)
{
    const struct m_obj_list *obj_list = NULL;
    enum mp_frame_type frame_type = 0;
    if (type == MP_OUTPUT_CHAIN_VIDEO) {
        frame_type = MP_FRAME_VIDEO;
        obj_list = &vf_obj_list;
    } else if (type == MP_OUTPUT_CHAIN_AUDIO) {
        frame_type = MP_FRAME_AUDIO;
        obj_list = &af_obj_list;
    }
    mp_assert(frame_type && obj_list);

    struct mp_filter *f = NULL;

    struct m_obj_desc desc;
    if (!m_obj_list_find(&desc, obj_list, bstr0(name)))
        goto done;

    void *options = NULL;
    if (desc.options) {
        struct m_config *config =
            m_config_from_obj_desc_and_args(NULL, parent->log, parent->global,
                                            &desc, args);

        if (!config)
            goto done;

        options = config->optstruct;
        // Free config when options is freed.
        ta_set_parent(options, NULL);
        ta_set_parent(config, options);
    }

    const struct mp_user_filter_entry *entry = desc.p;
    f = entry->create(parent, options);

done:
    if (!f) {
        MP_ERR(parent, "Creating filter '%s' failed.\n", name);
        return NULL;
    }
    return f;
}
