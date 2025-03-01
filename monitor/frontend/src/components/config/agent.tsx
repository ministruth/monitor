import GeoIP from '@/common/components/geoip';
import confirm from '@/common_components/layout/modal';
import Table from '@/common_components/layout/table';
import {
  IDColumn,
  SearchColumn,
  StatusColumn,
} from '@/common_components/layout/table/column';
import TableDelete from '@/common_components/layout/table/deleteBtn';
import styles from '@/common_components/layout/table/style.less';
import TableBtn from '@/common_components/layout/table/tableBtn';
import { API_PREFIX, PLUGIN_ID } from '@/config';
import {
  StringIntl,
  UserPerm,
  checkPerm,
  deleleAPI,
  getAPI,
  getIntl,
  postAPI,
} from '@/utils';
import { ReloadOutlined } from '@ant-design/icons';
import ProCard from '@ant-design/pro-card';
import { ParamsType, ProDescriptions } from '@ant-design/pro-components';
import { ActionType, ProColumns } from '@ant-design/pro-table';
import { FormattedMessage, useModel } from '@umijs/max';
import { Button } from 'antd';
import { SortOrder } from 'antd/es/table/interface';
import { Key, useRef, useState } from 'react';
import AgentUpdate from './agentUpdateBtn';
import PassiveAgent from './passiveBtn';

const request = async (params?: ParamsType, _?: Record<string, SortOrder>) => {
  const msg = await getAPI(`${API_PREFIX}/agents`, {
    text: params?.text,
    status: params?.status,
    page: params?.current,
    size: params?.pageSize,
  });
  return {
    data: msg.data.data,
    success: true,
    total: msg.data.total,
  };
};

const handleReconnect = (
  intl: StringIntl,
  ref: React.MutableRefObject<ActionType | undefined>,
  id: string,
  name: string,
) => {
  confirm({
    title: intl.get('pages.config.agent.reconnect.title', {
      name: name,
    }),
    content: intl.get('app.confirm'),
    onOk() {
      return new Promise((resolve, reject) => {
        postAPI(`${API_PREFIX}/agents/${id}/reconnect`, {}).then((rsp) => {
          if (rsp && rsp.code === 0) {
            ref.current?.reloadAndRest?.();
            resolve(rsp);
          } else {
            reject(rsp);
          }
        });
      });
    },
    intl: intl,
  });
};

const handleDeleteSelected = async (
  intl: StringIntl,
  ref: React.MutableRefObject<ActionType | undefined>,
  keys: Key[],
) => {
  confirm({
    title: intl.get('pages.config.agent.delete.selected.title'),
    content: intl.get('app.confirm'),
    onOk() {
      return new Promise((resolve, reject) => {
        deleleAPI(`${API_PREFIX}/agents`, { id: keys }).then((rsp) => {
          if (rsp && rsp.code === 0) {
            ref.current?.reloadAndRest?.();
            resolve(rsp);
          } else {
            reject(rsp);
          }
        });
      });
    },
    intl: intl,
  });
};

const AgentCard = () => {
  const intl = getIntl();
  const ref = useRef<ActionType>();
  const { access } = useModel('@@qiankunStateFromMaster');
  const statusEnum: { [Key: number]: { label: string; color: string } } = {
    0: {
      label: intl.get('tables.status.offline'),
      color: 'default',
    },
    1: {
      label: intl.get('tables.status.online'),
      color: 'success',
    },
    2: {
      label: intl.get('tables.status.updating'),
      color: 'warning',
    },
  };
  const columns: ProColumns[] = [
    SearchColumn(intl),
    IDColumn(intl),
    {
      title: intl.get('tables.name'),
      dataIndex: 'name',
      align: 'center',
      hideInSearch: true,
      ellipsis: true,
      onCell: () => {
        return {
          style: {
            maxWidth: 200,
          },
        };
      },
    },
    {
      title: intl.get('tables.ip'),
      dataIndex: 'ip',
      align: 'center',
      hideInSearch: true,
      render: (_, row) => <GeoIP value={row.ip} />,
    },
    {
      title: intl.get('tables.os'),
      dataIndex: 'os',
      align: 'center',
      hideInSearch: true,
    },
    {
      title: intl.get('tables.arch'),
      dataIndex: 'arch',
      align: 'center',
      hideInSearch: true,
    },
    StatusColumn(intl.get('tables.status'), 'status', statusEnum),
    {
      title: intl.get('tables.lastlogin'),
      dataIndex: 'last_login',
      align: 'center',
      valueType: 'dateTime',
      hideInSearch: true,
    },
    {
      title: intl.get('app.op'),
      valueType: 'option',
      align: 'center',
      className: styles.operation,
      width: 100,
      render: (_, row) => {
        return [
          <AgentUpdate
            key="update"
            tableRef={ref}
            initialValues={{
              ...row,
            }}
          />,

          <TableBtn
            key="reconnect"
            icon={ReloadOutlined}
            tip={intl.get('pages.config.agent.reconnect.tip')}
            color="#faad14"
            perm={UserPerm.PermWrite}
            permName={`manage.${PLUGIN_ID}`}
            onClick={() => handleReconnect(intl, ref, row.id, row.name)}
            disabled={row.status != 1}
          />,

          <TableDelete
            key="delete"
            permName={`manage.${PLUGIN_ID}`}
            perm={UserPerm.PermWrite}
            tableRef={ref}
            url={`${API_PREFIX}/agents/${row.id}`}
            confirmTitle={intl.get('pages.config.agent.delete.title', {
              name: row.name,
            })}
          />,
        ];
      },
    },
  ];

  const [selectedRowKeys, setSelectedRowKeys] = useState<Key[]>([]);
  const onSelectChange = (keys: Key[]) => {
    setSelectedRowKeys(keys);
  };
  const rowSelection = {
    selectedRowKeys,
    onChange: onSelectChange,
  };
  return (
    <ProCard title={intl.get('pages.config.agent.title')} bordered>
      <Table
        actionRef={ref}
        rowKey="id"
        rowSelection={rowSelection}
        tableAlertRender={false}
        request={request}
        columns={columns}
        action={[
          <PassiveAgent />,
          <Button
            key="delete"
            danger
            disabled={
              !checkPerm(access, `manage.${PLUGIN_ID}`, UserPerm.PermWrite) ||
              selectedRowKeys.length === 0
            }
            onClick={() => handleDeleteSelected(intl, ref, selectedRowKeys)}
          >
            <FormattedMessage id="app.op.delete" />
          </Button>,
        ]}
        expandable={{
          expandRowByClick: true,
          expandedRowRender: (record: any) => {
            return (
              <ProDescriptions
                column={2}
                dataSource={record}
                columns={[
                  {
                    title: intl.get('tables.uid'),
                    dataIndex: 'uid',
                    style: { paddingBottom: 0 },
                    copyable: true,
                  },
                  {
                    title: intl.get('tables.hostname'),
                    dataIndex: 'hostname',
                    style: { paddingBottom: 0 },
                  },
                  {
                    title: intl.get('tables.system'),
                    dataIndex: 'system',
                    style: { paddingBottom: 0 },
                  },
                  {
                    title: intl.get('tables.address'),
                    dataIndex: 'address',
                    style: { paddingBottom: 0 },
                  },
                  {
                    title: intl.get('tables.endpoint'),
                    dataIndex: 'endpoint',
                    style: { paddingBottom: 0 },
                  },
                ]}
              />
            );
          },
        }}
      />
    </ProCard>
  );
};

export default AgentCard;
